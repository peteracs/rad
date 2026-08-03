use super::*;

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;

use crate::gc::GcHeap;
use crate::value::{Builtin, MapKey, MapStorage, Value};

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
        // Replay interposition: managed builtins never execute â€” their
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
        // The event-buffer flip is the frame boundary for record & replay â€”
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
            // is keyframed â€” so the scrubbed frame N is the first one
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
            // frame current+1" â€” keyframe it before advancing.
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

    fn expect_component_type_name(arg: &Value, fn_name: &str) -> Result<String, String> {
        if let Some(name) = arg.as_str() {
            return Ok(name.to_string());
        }
        if let Some(comp) = arg.as_component() {
            return Ok(comp.type_name.clone());
        }
        Err(format!(
            "{}() expects component type string or component value, got {}",
            fn_name,
            arg.type_name()
        ))
    }

    fn bi_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("get() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("get() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "get")?;
        self.sandbox_check_read(&ctype)?;
        match self.world.get_component(eid, &ctype) {
            Some(comp) => {
                let mut fields = HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_component_data(&mut self.gc, comp),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                HashMap::new(),
            )),
        }
    }

    fn bi_get_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("get_resource() requires 1 argument".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "get_resource")?;
        self.sandbox_check_read(&rtype)?;
        match self.world.get_resource(&rtype) {
            Some(comp) => {
                let mut fields = HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_component_data(&mut self.gc, comp),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                HashMap::new(),
            )),
        }
    }

    /// `res(R) -> R` — direct resource access. Declared resources are
    /// auto-initialized from their field defaults, so the Option dance of
    /// `get_resource(R) |> unwrap` is pure ceremony; this is the shorthand.
    fn bi_res(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("res() requires 1 argument (the resource type)".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "res")?;
        self.sandbox_check_read(&rtype)?;
        match self.world.get_resource(&rtype) {
            Some(comp) => Ok(Value::from_component_data(&mut self.gc, comp)),
            None => Err(format!(
                "res() found no resource '{}' — is it declared with `resource {} {{ ... }}`?",
                rtype, rtype
            )),
        }
    }

    /// `recent_events(name, window) -> list` — payloads of every `name`
    /// event dispatched within the last `window` flush cycles (game
    /// ticks), oldest first. The queryable past: death recaps, combat
    /// windows, "what hit me" panels — straight off the deterministic
    /// event log instead of a hand-rolled ring buffer.
    fn bi_recent_events(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("recent_events() requires 2 arguments (event name, window ticks)".into());
        }
        let Some(name) = args[0].as_str() else {
            return Err(format!(
                "recent_events() event name must be a string, got {}",
                args[0].type_name()
            ));
        };
        let Some(window) = args[1].as_int() else {
            return Err(format!(
                "recent_events() window must be an int tick count, got {}",
                args[1].type_name()
            ));
        };
        let since = self.causality_frame.saturating_sub(window.max(0) as u64);
        let payloads: Vec<Value> = self
            .event_log
            .iter()
            .filter(|e| e.event_name == name && e.tick >= since)
            .map(|e| e.payload)
            .collect();
        Ok(Value::list(&mut self.gc, payloads))
    }

    fn bi_lookup(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err("lookup() requires 3 arguments".into());
        }
        let ctype = Self::expect_component_type_name(&args[0], "lookup")?;
        self.sandbox_check_read(&ctype)?;
        let field = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "lookup() second argument must be a field name string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        if !self.world.is_field_indexed(&ctype, &field) {
            return Err(format!(
                "lookup() requires an indexed field: '{}.{}' is not indexed",
                ctype, field
            ));
        }
        match self.world.index_lookup(&ctype, &field, args[2]) {
            Some(eid) => {
                let mut fields = HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_entity_id(&mut self.gc, eid),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                HashMap::new(),
            )),
        }
    }

    /// `lookup_all(Comp, "field", value) -> list<entity>` — every entity
    /// whose indexed field equals the value, ids ascending (deterministic
    /// across save/load and replay). The multi-match sibling of `lookup`:
    /// "all open tickets" is one hash probe instead of an O(world) scan.
    fn bi_lookup_all(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err("lookup_all() requires 3 arguments".into());
        }
        let ctype = Self::expect_component_type_name(&args[0], "lookup_all")?;
        self.sandbox_check_read(&ctype)?;
        let field = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "lookup_all() second argument must be a field name string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        if !self.world.is_field_indexed(&ctype, &field) {
            return Err(format!(
                "lookup_all() requires an indexed field: '{}.{}' is not indexed",
                ctype, field
            ));
        }
        let ids = self.world.index_lookup_all(&ctype, &field, args[2]);
        let vals: Vec<Value> = ids
            .into_iter()
            .map(|eid| Value::from_entity_id(&mut self.gc, eid))
            .collect();
        Ok(Value::list(&mut self.gc, vals))
    }

    fn bi_require(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("require() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("require() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "require")?;
        self.sandbox_check_read(&ctype)?;
        match self.world.get_component(eid, &ctype) {
            Some(comp) => Ok(Value::from_component_data(&mut self.gc, comp)),
            None => {
                // The teaching error: who, what's missing, what's actually
                // there. A raw entity id helps nobody.
                let who = self
                    .world
                    .entity_name(eid)
                    .map(|n| format!("'{}'", n))
                    .unwrap_or_else(|| format!("entity {}", eid));
                if !self.world.contains_entity(eid) {
                    return Err(format!(
                        "require() on {}: entity no longer exists (despawned?)",
                        who
                    ));
                }
                let mut has: Vec<String> = self
                    .world
                    .components_on_entity(eid)
                    .iter()
                    .map(|c| c.type_name.clone())
                    .collect();
                has.sort();
                Err(format!(
                    "require() missing component '{}' on {} (has: [{}])",
                    ctype,
                    who,
                    has.join(", ")
                ))
            }
        }
    }

    fn bi_require_all(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("require_all() requires at least 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("require_all() expects entity, got {}", args[0].type_name()))?;
        let mut out = Vec::with_capacity(args.len() - 1);
        for arg in args.iter().skip(1) {
            let ctype = Self::expect_component_type_name(arg, "require_all")?;
            self.sandbox_check_read(&ctype)?;
            match self.world.get_component(eid, &ctype) {
                Some(comp) => out.push(Value::from_component_data(&mut self.gc, comp)),
                None => {
                    return Err(format!(
                        "require_all() missing component '{}' on entity {}",
                        ctype, eid
                    ));
                }
            }
        }
        Ok(Value::list(&mut self.gc, out))
    }

    fn bi_set(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("set() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("set() expects entity, got {}", args[0].type_name()))?;
        let data = args[1]
            .as_component()
            .ok_or_else(|| format!("set() expects component, got {}", args[1].type_name()))?;
        self.sandbox_check_write(&data.type_name)?;
        self.sandbox_check_write_shape(data)?;
        // One persist: the command buffer owns deferred values (they must
        // survive worker GC); the direct path hands ownership to the world
        // via the owned sink. Persisting on both sides of either path
        // abandons a full persistent copy per write.
        let mut data = data.clone();
        Value::persist_component_data(&mut data);
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::SetComponent(eid, data));
        } else {
            let cname = data.type_name.clone();
            let summary = Self::component_summary(&data);
            if !self.world.add_component_owned(eid, data) {
                return Err(format!("set() called on non-existent entity {}", eid));
            }
            self.record_causal_write(Some(eid), &cname, crate::causality::WriteKind::Set, summary);
        }
        Ok(Value::NIL)
    }

    fn bi_set_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("set_resource() requires 2 arguments".into());
        }
        let rtype = Self::expect_component_type_name(&args[0], "set_resource")?;
        self.sandbox_check_write(&rtype)?;
        let data = args[1].as_component().ok_or_else(|| {
            format!(
                "set_resource() expects component, got {}",
                args[1].type_name()
            )
        })?;
        self.sandbox_check_write_shape(data)?;
        let data = data.clone();
        if self.is_worker {
            let mut buffered = data.clone();
            Value::persist_component_data(&mut buffered);
            self.command_buffer
                .push(crate::vm::EcsCommand::SetResource(rtype.clone(), buffered));
            // A resource is shared by every entity the system visits, so the
            // worker's private world must observe the write — otherwise the
            // next iteration reads the pre-batch snapshot and the buffered
            // absolute values all collapse to a single step.
            self.world.set_resource(&rtype, data);
        } else {
            let mut data = data;
            Value::persist_component_data(&mut data);
            let summary = Self::component_summary(&data);
            self.world.set_resource_owned(&rtype, data);
            self.record_causal_write(None, &rtype, crate::causality::WriteKind::Resource, summary);
        }
        Ok(Value::NIL)
    }

    fn bi_has(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("has() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("has() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "has")?;
        self.sandbox_check_read(&ctype)?;
        Ok(Value::from_bool(self.world.has_component(eid, &ctype)))
    }

    fn bi_spawn(&mut self, args: Vec<Value>) -> Result<Value, String> {
        // ACL before any mutation: the entity must not be spawned if any
        // component in the argument list is outside the capability grant,
        // carries a shape the host did not declare, or would shadow an
        // existing entity name.
        if self.sandbox_caps.is_some() {
            // Entity-name squatting (list item #2): a guest must not spawn
            // under a name that already resolves to a host entity. A
            // duplicate name silently reassigns the registry to the new
            // entity and orphans the old one, so the host's later
            // get_entity(name) would operate on guest-controlled data —
            // while diff/assert_only_changed, which do not cover the name
            // registry, report only the newly written component. Deny it.
            if let Some(name) = args.first().and_then(|v| v.as_str()) {
                if !name.is_empty() && self.world.get_entity_by_name(name).is_some() {
                    return Err(format!(
                        "sandbox: spawn(\"{}\", ...) denied — an entity named '{}' already \
                         exists; a sandboxed guest may not shadow an existing entity name",
                        name, name
                    ));
                }
            }
            for arg in &args {
                if let Some(c) = arg.as_component() {
                    self.sandbox_check_write(&c.type_name)?;
                    self.sandbox_check_write_shape(c)?;
                }
            }
        }
        let name = args.first().and_then(|v| v.as_str().map(|s| s.to_string()));
        let eid = self.world.spawn_entity(name.as_deref());
        let start_idx = if name.is_some() { 1 } else { 0 };

        if self.is_worker {
            let mut comps = Vec::new();
            for arg in args.iter().skip(start_idx) {
                if let Some(c) = arg.as_component() {
                    let mut data = c.clone();
                    Value::persist_component_data(&mut data);
                    comps.push(data);
                }
            }
            self.command_buffer
                .push(crate::vm::EcsCommand::SpawnEntity(name, comps, eid));
        } else {
            for arg in args.into_iter().skip(start_idx) {
                if let Some(c) = arg.as_component() {
                    let data = c.clone();
                    let cname = data.type_name.clone();
                    let summary = Self::component_summary(&data);
                    // add_component persists; pre-persisting here would
                    // abandon a copy per spawned component.
                    let _ = self.world.add_component(eid, data);
                    self.record_causal_write(
                        Some(eid),
                        &cname,
                        crate::causality::WriteKind::Spawn,
                        summary,
                    );
                }
            }
        }
        Ok(Value::from_entity_id(&mut self.gc, eid))
    }

    fn bi_get_entity(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("get_entity() requires 1 argument".into());
        }
        let name = args[0].as_str().ok_or_else(|| {
            format!(
                "get_entity() expects string name, got {}",
                args[0].type_name()
            )
        })?;
        if let Some(eid) = self.world.get_entity_by_name(name) {
            Ok(Value::from_entity_id(&mut self.gc, eid))
        } else {
            Ok(Value::NIL)
        }
    }

    /// `require_entity(name) -> entity` — the fail-fast dual of
    /// `get_entity` (same pairing as get/require for components).
    fn bi_require_entity(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("require_entity() requires 1 argument".into());
        }
        let name = args[0].as_str().ok_or_else(|| {
            format!(
                "require_entity() expects string name, got {}",
                args[0].type_name()
            )
        })?;
        match self.world.get_entity_by_name(name) {
            Some(eid) => Ok(Value::from_entity_id(&mut self.gc, eid)),
            None => Err(format!("require_entity(): no entity named '{}'", name)),
        }
    }

    /// `name_of(entity) -> str` — the inverse of `get_entity`. Anonymous
    /// entities yield "" (matching how summaries render unnamed ids).
    fn bi_name_of(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("name_of() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("name_of() expects entity, got {}", args[0].type_name()))?;
        let name = self.world.entity_name(eid).unwrap_or_default();
        Ok(Value::from_string(&mut self.gc, name))
    }

    fn bi_id_of(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("id_of() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("id_of() expects entity, got {}", args[0].type_name()))?;
        Ok(Value::from_int(&mut self.gc, eid as i64))
    }

    fn bi_remove(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("remove() requires 2 arguments".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("remove() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "remove")?;
        self.sandbox_check_write(&ctype)?;
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::RemoveComponent(eid, ctype.clone()));
            Ok(Value::from_bool(true))
        } else {
            let removed = self.world.remove_component(eid, &ctype);
            if removed {
                self.record_causal_write(
                    Some(eid),
                    &ctype,
                    crate::causality::WriteKind::Remove,
                    String::new(),
                );
            }
            Ok(Value::from_bool(removed))
        }
    }

    fn bi_despawn(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            return Err("despawn() requires 1 argument".into());
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("despawn() expects entity, got {}", args[0].type_name()))?;
        self.sandbox_check_despawn()?;
        if self.is_worker {
            self.command_buffer
                .push(crate::vm::EcsCommand::DespawnEntity(eid));
            Ok(Value::from_bool(true))
        } else {
            // Record before destroy: the entity's name is wiped with it.
            self.record_causal_write(
                Some(eid),
                "*",
                crate::causality::WriteKind::Despawn,
                String::new(),
            );
            if !self.world.destroy_entity(eid) {
                return Err(format!("despawn() called on non-existent entity {}", eid));
            }
            Ok(Value::from_bool(true))
        }
    }

    fn bi_entities(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            // Unfiltered `entities()` enumerates every entity in the world
            // regardless of component, so it cannot be keyed to a read grant
            // and requires the wildcard.
            self.sandbox_check_bulk_read("entities()")?;
            let ids = self.world.all_entity_ids();
            let mut vals = Vec::with_capacity(ids.len());
            for id in ids {
                vals.push(Value::from_entity_id(&mut self.gc, id));
            }
            Ok(Value::list(&mut self.gc, vals))
        } else {
            let ctypes: Result<Vec<String>, String> = args
                .iter()
                .map(|arg| Self::expect_component_type_name(arg, "entities"))
                .collect();
            let ctypes = ctypes?;
            for ctype in &ctypes {
                self.sandbox_check_read(ctype)?;
            }
            let ids = self.world.query(&ctypes, &[]);
            let mut vals = Vec::with_capacity(ids.len());
            for id in ids {
                vals.push(Value::from_entity_id(&mut self.gc, id));
            }
            Ok(Value::list(&mut self.gc, vals))
        }
    }

    fn bi_transition(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("transition() requires 2 arguments".into());
        }
        let s = args[0]
            .as_state()
            .ok_or_else(|| format!("transition() expects state, got {}", args[0].type_name()))?;
        let machine = s.machine.clone();
        let state = s.state.clone();
        let event = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "transition() expects event string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        self.transition_result(machine, state, event)
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

    fn bi_buffer_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "buffer_new() takes no arguments, got {}",
                args.len()
            ));
        }
        Ok(Value::buffer(&mut self.gc, String::new()))
    }

    fn bi_buffer_append(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "buffer_append() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let s_val = args.pop().unwrap();
        let buf_val = args.pop().unwrap();

        let s = s_val
            .as_str()
            .ok_or_else(|| "buffer_append() second argument must be a string".to_string())?;

        let mut buf = buf_val
            .into_buffer()
            .ok_or_else(|| "buffer_append() first argument must be a buffer".to_string())?;

        buf.push_str(s);
        Ok(Value::buffer(&mut self.gc, buf))
    }

    fn bi_buffer_to_str(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "buffer_to_str() expects 1 argument, got {}",
                args.len()
            ));
        }
        let buf = args[0]
            .as_buffer()
            .ok_or_else(|| "buffer_to_str() argument must be a buffer".to_string())?;
        Ok(Value::from_string(&mut self.gc, buf.clone()))
    }

    fn bi_bytebuf_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_new() expects 1 argument, got {}",
                args.len()
            ));
        }
        let size = bytebuf_index_arg(&args[0], "bytebuf_new() size")?;
        Ok(Value::bytebuf(&mut self.gc, vec![0; size]))
    }

    fn bi_bytebuf_len(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_len() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_len() expects a bytebuf".to_string())?;
        Ok(Value::from_int(&mut self.gc, bytes.len() as i64))
    }

    fn bi_bytebuf_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "bytebuf_get() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_get() expects a bytebuf".to_string())?;
        let idx = bytebuf_index_arg(&args[1], "bytebuf_get() index")?;
        if idx >= bytes.len() {
            return Err(format!(
                "bytebuf_get() index {} out of bounds (len {})",
                idx,
                bytes.len()
            ));
        }
        Ok(Value::from_int(&mut self.gc, i64::from(bytes[idx])))
    }

    fn bi_bytebuf_set_u8(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "bytebuf_set_u8() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let mut bytes = args[0]
            .into_bytebuf()
            .ok_or_else(|| "bytebuf_set_u8() expects a bytebuf".to_string())?;
        let idx = bytebuf_index_arg(&args[1], "bytebuf_set_u8() index")?;
        let byte = bytebuf_u8_arg(&args[2], "bytebuf_set_u8() value")?;
        if idx >= bytes.len() {
            return Err(format!(
                "bytebuf_set_u8() index {} out of bounds (len {})",
                idx,
                bytes.len()
            ));
        }
        bytes[idx] = byte;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_bytebuf_set_u32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_set_u32_or_i32_le(args, "bytebuf_set_u32_le()")
    }

    fn bi_bytebuf_set_i32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_set_u32_or_i32_le(args, "bytebuf_set_i32_le()")
    }

    fn bi_bytebuf_set_u32_or_i32_le(
        &mut self,
        args: Vec<Value>,
        fn_name: &str,
    ) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "{} expects 3 arguments, got {}",
                fn_name,
                args.len()
            ));
        }
        let mut bytes = args[0]
            .into_bytebuf()
            .ok_or_else(|| format!("{} expects a bytebuf", fn_name))?;
        let offset = bytebuf_index_arg(&args[1], &format!("{} offset", fn_name))?;
        let value = args[2]
            .as_int()
            .ok_or_else(|| format!("{} expects int value", fn_name))?;
        bytebuf_write_u32_le(&mut bytes, offset, value as u32, fn_name)?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_bytebuf_get_u32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_get_u32_or_i32_le(args, false, "bytebuf_get_u32_le()")
    }

    fn bi_bytebuf_get_i32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_get_u32_or_i32_le(args, true, "bytebuf_get_i32_le()")
    }

    fn bi_bytebuf_get_u32_or_i32_le(
        &mut self,
        args: Vec<Value>,
        signed: bool,
        fn_name: &str,
    ) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "{} expects 2 arguments, got {}",
                fn_name,
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| format!("{} expects a bytebuf", fn_name))?;
        let offset = bytebuf_index_arg(&args[1], &format!("{} offset", fn_name))?;
        let value = bytebuf_read_u32_le(bytes, offset, fn_name)?;
        let result = if signed {
            i64::from(value as i32)
        } else {
            i64::from(value)
        };
        Ok(Value::from_int(&mut self.gc, result))
    }

    fn bi_bytebuf_to_list(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_to_list() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_to_list() expects a bytebuf".to_string())?;
        let mut values = Vec::with_capacity(bytes.len());
        for byte in bytes {
            values.push(Value::from_int(&mut self.gc, i64::from(*byte)));
        }
        Ok(Value::list(&mut self.gc, values))
    }

    fn bi_bytebuf_from_list(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_from_list() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = bytes_from_list_arg(&args[0], "bytebuf_from_list()")?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_fork(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!("fork() takes no arguments, got {}", args.len()));
        }
        // Full program state: world + in-flight events. A fork that drops
        // pending events is not a fork (composition pass, #7).
        let snapshot = self.snapshot_with_events();
        Ok(Value::world_fork(
            &mut self.gc,
            std::sync::Arc::new(snapshot),
        ))
    }

    fn bi_simulate(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "simulate() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "simulate() first argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        let system_names: Vec<String> = {
            let list = args[1].as_list().ok_or_else(|| {
                "simulate() second argument must be a list of systems".to_string()
            })?;
            list.iter()
                .map(|v| {
                    v.as_system_ref().map(|s| s.to_string()).ok_or_else(|| {
                        format!(
                            "simulate() schedule must be a list of `system` values, got {}",
                            v.type_name()
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let ticks = args[2]
            .as_int()
            .ok_or_else(|| "simulate() third argument must be an integer".to_string())?;
        if ticks < 0 {
            return Err("simulate() tick count must be non-negative".to_string());
        }

        let saved_world = std::mem::take(self.get_world_mut());
        let saved_events_current = std::mem::take(&mut self.events_current);
        let saved_events_next = std::mem::take(&mut self.events_next);
        let saved_emit_ids_current = std::mem::take(&mut self.emit_ids_current);
        let saved_emit_ids_next = std::mem::take(&mut self.emit_ids_next);
        // delayed (`emit … after N`) queues swap too: sim ticks must not
        // age the live queue. The fork's own timers seed via
        // restore_events_from; sim leftovers ride the result snapshot.
        let saved_delayed = std::mem::take(&mut self.delayed_events);

        // The saved timeline's event payloads now live in Rust locals where
        // the collector cannot see them: auto-GC stays off until they are
        // restored, or the simulation sweeps the main timeline's pending
        // events out from under it (web arena crash, 1-in-3 runs).
        self.gc_pause += 1;

        // The fork's pending events run inside the simulation â€” they are
        // part of the state being speculated on, not main-timeline residue.
        self.restore_events_from(&fork_snap);
        self.get_world_mut().restore(fork_snap);
        self.in_simulation_fork += 1;

        let sim_result = (|| -> Result<(), String> {
            for _ in 0..ticks {
                for name in &system_names {
                    self.run_system_by_name(name)?;
                }
                self.bi_flush_events(vec![])?;
            }
            Ok(())
        })();

        self.in_simulation_fork -= 1;

        // Whatever the simulation left in flight travels with the result.
        let new_snapshot = self.snapshot_with_events();

        *self.get_world_mut() = saved_world;
        self.events_current = saved_events_current;
        self.events_next = saved_events_next;
        self.emit_ids_current = saved_emit_ids_current;
        self.emit_ids_next = saved_emit_ids_next;
        self.delayed_events = saved_delayed;
        self.gc_pause -= 1;

        sim_result?;
        Ok(Value::world_fork(
            &mut self.gc,
            std::sync::Arc::new(new_snapshot),
        ))
    }

    fn bi_commit(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!("commit() expects 1 argument, got {}", args.len()));
        }
        let fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "commit() argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        // The snapshot's pending events come back with it â€” commit restores
        // the *whole* program state, it does not launder the event queue.
        self.restore_events_from(&fork_snap);
        // Foreign provenance riding the fork (it crossed a wire) lands in
        // the local ledger now â€” adopting a timeline adopts its history.
        // Foreign emit ids are remapped to fresh local ids, including the
        // in-flight queue's, so handler writes that fire *after* this
        // commit still chain back to the remote emit records.
        if self.in_simulation_fork == 0 {
            if let Some(prov) = fork_snap.provenance.as_deref() {
                let id_map = self.ledger.ingest(prov, &std::collections::HashMap::new());
                for id in self.emit_ids_next.iter_mut() {
                    if let Some(&local) = id_map.get(id) {
                        *id = local;
                    }
                }
                for (_, _, _, id) in self.delayed_events.iter_mut() {
                    if let Some(&local) = id_map.get(id) {
                        *id = local;
                    }
                }
            }
        }
        self.get_world_mut().restore(fork_snap);
        // The program's `indexed` declarations are the source of truth;
        // snapshots carry only derived state. A snapshot from a foreign
        // lineage (old save, pre-fix wire decode) must not wipe the live
        // world's indexes — reconcile (no-op when they already agree).
        let decl = std::sync::Arc::clone(&self.indexed_decl);
        self.get_world_mut().ensure_indexed_fields(&decl);
        // Causality seam: provenance recorded before this point describes
        // the pre-fork timeline. `why()` discloses that honestly.
        if self.in_simulation_fork == 0 {
            self.ledger.record_commit(self.causality_frame);
        }
        Ok(Value::NIL)
    }

    /// `merge_forks(base, ours, theirs) -> Result<world_fork, str>` â€”
    /// three-way world merge (#7). Field-level: a conflict is the *same
    /// field* of the same entity/resource diverging from base in both forks.
    /// Id collisions between independent spawns are remapped (with deep
    /// reference rewriting), not conflicted; name collisions are honest
    /// conflicts. `commit()` the Ok fork to adopt the merged timeline.
    fn bi_merge_forks(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "merge_forks() expects 3 arguments (base, ours, theirs), got {}",
                args.len()
            ));
        }
        let snaps: Vec<std::sync::Arc<crate::world::WorldSnapshot>> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.as_world_fork().cloned().ok_or_else(|| {
                    format!(
                        "merge_forks() argument {} must be a world_fork, got {}",
                        i + 1,
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        self.run_merge(&snaps, crate::merge::Resolutions::default())
    }

    /// `merge_forks_with(base, ours, theirs, resolutions) -> Result<world_fork, list<Conflict>>`
    /// â€” the programmable half of conflicts-as-data. `resolutions` is a list
    /// of `(conflict, value)` pairs: each field conflict named by the pair
    /// merges as the given value instead of refusing; a NameConflict takes a
    /// list of new names (one per claiming entity) and a RenameConflict takes
    /// the chosen name. Despawn and event conflicts are not mechanically
    /// resolvable.
    fn bi_merge_forks_with(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "merge_forks_with() expects 4 arguments (base, ours, theirs, resolutions), got {}",
                args.len()
            ));
        }
        let snaps: Vec<std::sync::Arc<crate::world::WorldSnapshot>> = args[..3]
            .iter()
            .enumerate()
            .map(|(i, a)| {
                a.as_world_fork().cloned().ok_or_else(|| {
                    format!(
                        "merge_forks_with() argument {} must be a world_fork, got {}",
                        i + 1,
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        let list = args[3].as_list().ok_or_else(|| {
            format!(
                "merge_forks_with() resolutions must be a list of (conflict, value) pairs, got {}",
                args[3].type_name()
            )
        })?;
        let mut resolutions = crate::merge::Resolutions::default();
        for (i, pair) in list.iter().enumerate() {
            let items = pair.as_tuple().ok_or_else(|| {
                format!(
                    "merge_forks_with() resolution {} must be a (conflict, value) tuple, got {}",
                    i,
                    pair.type_name()
                )
            })?;
            if items.len() != 2 {
                return Err(format!(
                    "merge_forks_with() resolution {} must be a (conflict, value) pair",
                    i
                ));
            }
            let st = items[0].as_sum_type().ok_or_else(|| {
                format!(
                    "merge_forks_with() resolution {}: first element must be a Conflict, got {}",
                    i,
                    items[0].type_name()
                )
            })?;
            if st.type_name != "Conflict" {
                return Err(format!(
                    "merge_forks_with() resolution {}: expected a Conflict, got {}",
                    i, st.type_name
                ));
            }
            let key = match st.variant.as_str() {
                "FieldConflict" => {
                    let eid = st
                        .fields
                        .get("ent")
                        .and_then(|v| v.as_entity_id())
                        .ok_or("merge_forks_with(): FieldConflict missing entity")?;
                    let component = st
                        .fields
                        .get("comp")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): FieldConflict missing component")?;
                    let field = st
                        .fields
                        .get("field")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): FieldConflict missing field")?;
                    (Some(eid), component, field)
                }
                "ResourceFieldConflict" => {
                    let resource = st
                        .fields
                        .get("res")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): ResourceFieldConflict missing resource")?;
                    let field = st
                        .fields
                        .get("field")
                        .and_then(|v| v.as_str().map(String::from))
                        .ok_or("merge_forks_with(): ResourceFieldConflict missing field")?;
                    (None, resource, field)
                }
                // Name claims are resolvable by *renaming*: the value is a
                // list of names parallel to the conflict's `entities` list
                // ("keep both as T-5/a, T-5/b"). "" unnames. The merge
                // re-validates: chosen names that still collide come back
                // as conflicts, so a rename can never steal a name unnoticed.
                "NameConflict" => {
                    let ents = st
                        .fields
                        .get("entities")
                        .and_then(|v| v.as_list().map(|l| l.to_vec()))
                        .ok_or("merge_forks_with(): NameConflict missing entities")?;
                    let names = items[1].as_list().ok_or_else(|| {
                        format!(
                            "merge_forks_with() resolution {}: a NameConflict resolution \
                             must be a list of new names (one per claiming entity), got {}",
                            i,
                            items[1].type_name()
                        )
                    })?;
                    if names.len() != ents.len() {
                        return Err(format!(
                            "merge_forks_with() resolution {}: NameConflict has {} claiming \
                             entities but {} names were given (one name per entity, \
                             \"\" to unname)",
                            i,
                            ents.len(),
                            names.len()
                        ));
                    }
                    for (ent, name) in ents.iter().zip(names.iter()) {
                        let eid = ent.as_entity_id().ok_or(
                            "merge_forks_with(): NameConflict entities must be entity ids",
                        )?;
                        let n = name.as_str().ok_or_else(|| {
                            format!(
                                "merge_forks_with() resolution {}: NameConflict names must \
                                 be strings, got {}",
                                i,
                                name.type_name()
                            )
                        })?;
                        resolutions
                            .renames
                            .insert(eid, Some(n.to_string()).filter(|s| !s.is_empty()));
                    }
                    continue;
                }
                // Renamed-differently-in-both-forks: the value is the one
                // name the entity should carry.
                "RenameConflict" => {
                    let eid = st
                        .fields
                        .get("ent")
                        .and_then(|v| v.as_entity_id())
                        .ok_or("merge_forks_with(): RenameConflict missing entity")?;
                    let n = items[1].as_str().ok_or_else(|| {
                        format!(
                            "merge_forks_with() resolution {}: a RenameConflict resolution \
                             must be the chosen name (a str), got {}",
                            i,
                            items[1].type_name()
                        )
                    })?;
                    resolutions
                        .renames
                        .insert(eid, Some(n.to_string()).filter(|s| !s.is_empty()));
                    continue;
                }
                other => {
                    return Err(format!(
                        "merge_forks_with(): {} is not mechanically resolvable \
                         (field, resource-field, name-claim, and rename conflicts are; \
                         despawns and event consumption have no honest 'pick a side')",
                        other
                    ));
                }
            };
            resolutions.fields.insert(key, items[1]);
        }
        self.run_merge(&snaps, resolutions)
    }

    fn run_merge(
        &mut self,
        snaps: &[std::sync::Arc<crate::world::WorldSnapshot>],
        resolutions: crate::merge::Resolutions,
    ) -> Result<Value, String> {
        match crate::merge::merge_worlds(
            &snaps[0],
            &snaps[1],
            &snaps[2],
            &mut self.gc,
            &resolutions,
        ) {
            Ok(outcome) => {
                let mut snap = outcome.world.snapshot();
                // The merged in-flight event queue travels with the fork;
                // commit() will restore it. Never silently dropped.
                snap.events = std::sync::Arc::new(outcome.events);
                snap.emit_ids = std::sync::Arc::new(outcome.emit_ids);
                snap.delayed = std::sync::Arc::new(outcome.delayed);
                // Foreign provenance survives the merge: records from either
                // input ride the merged fork (theirs' entity ids follow the
                // spawn-collision remap), so commit() can stitch the remote
                // history into the local ledger.
                let remap: std::collections::HashMap<u32, u32> =
                    outcome.remapped.iter().copied().collect();
                let mut combined = crate::causality::WireProvenance::default();
                for (src, apply_remap) in [(&snaps[1], false), (&snaps[2], true)] {
                    if let Some(p) = src.provenance.as_deref() {
                        // Materialize each record's origin now — the two
                        // sides may have arrived from different machines.
                        let label = || {
                            Some(if p.origin.is_empty() {
                                "wire".to_string()
                            } else {
                                p.origin.clone()
                            })
                        };
                        for w in &p.writes {
                            let mut w = w.clone();
                            if apply_remap {
                                if let Some(e) = w.entity {
                                    w.entity = Some(remap.get(&e).copied().unwrap_or(e));
                                }
                            }
                            w.origin = w.origin.take().or_else(label);
                            combined.writes.push(w);
                        }
                        for e in &p.emits {
                            let mut e = e.clone();
                            e.origin = e.origin.take().or_else(label);
                            combined.emits.push(e);
                        }
                    }
                }
                if !combined.writes.is_empty() || !combined.emits.is_empty() {
                    snap.provenance = Some(std::sync::Arc::new(combined));
                }
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(conflicts) => {
                let items: Vec<Value> = conflicts
                    .iter()
                    .map(|c| self.conflict_to_value(c))
                    .collect();
                let v = Value::list(&mut self.gc, items);
                Ok(self.make_result(false, v))
            }
        }
    }

    /// One [`crate::merge::MergeConflict`] as a rad `Conflict` sum value â€”
    /// the boundary where merge conflicts become user-space data.
    fn conflict_to_value(&mut self, c: &crate::merge::MergeConflict) -> Value {
        use crate::merge::MergeConflict as MC;
        let mut fields = std::collections::HashMap::new();
        let variant = match c {
            MC::Field {
                entity,
                entity_name,
                component,
                field,
                base,
                ours,
                theirs,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                let n = entity_name.clone().unwrap_or_default();
                fields.insert("name".into(), Value::from_string(&mut self.gc, n));
                fields.insert(
                    "comp".into(),
                    Value::from_string(&mut self.gc, component.clone()),
                );
                fields.insert(
                    "field".into(),
                    Value::from_string(&mut self.gc, field.clone()),
                );
                fields.insert("base".into(), *base);
                fields.insert("ours".into(), *ours);
                fields.insert("theirs".into(), *theirs);
                "FieldConflict"
            }
            MC::Component {
                entity,
                entity_name,
                component,
                detail,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                let n = entity_name.clone().unwrap_or_default();
                fields.insert("name".into(), Value::from_string(&mut self.gc, n));
                fields.insert(
                    "comp".into(),
                    Value::from_string(&mut self.gc, component.clone()),
                );
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "ComponentConflict"
            }
            MC::Despawn {
                entity,
                entity_name,
                detail,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                let n = entity_name.clone().unwrap_or_default();
                fields.insert("name".into(), Value::from_string(&mut self.gc, n));
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "DespawnConflict"
            }
            MC::Rename {
                entity,
                base,
                ours,
                theirs,
            } => {
                fields.insert("ent".into(), Value::from_entity_id(&mut self.gc, *entity));
                fields.insert(
                    "base".into(),
                    Value::from_string(&mut self.gc, base.clone()),
                );
                fields.insert(
                    "ours".into(),
                    Value::from_string(&mut self.gc, ours.clone()),
                );
                fields.insert(
                    "theirs".into(),
                    Value::from_string(&mut self.gc, theirs.clone()),
                );
                "RenameConflict"
            }
            MC::NameClaim { name, entities } => {
                fields.insert(
                    "name".into(),
                    Value::from_string(&mut self.gc, name.clone()),
                );
                let ids: Vec<Value> = entities
                    .iter()
                    .map(|&e| Value::from_entity_id(&mut self.gc, e))
                    .collect();
                fields.insert("entities".into(), Value::list(&mut self.gc, ids));
                "NameConflict"
            }
            MC::ResourceField {
                resource,
                field,
                base,
                ours,
                theirs,
            } => {
                fields.insert(
                    "res".into(),
                    Value::from_string(&mut self.gc, resource.clone()),
                );
                fields.insert(
                    "field".into(),
                    Value::from_string(&mut self.gc, field.clone()),
                );
                fields.insert("base".into(), *base);
                fields.insert("ours".into(), *ours);
                fields.insert("theirs".into(), *theirs);
                "ResourceFieldConflict"
            }
            MC::Resource { resource, detail } => {
                fields.insert(
                    "res".into(),
                    Value::from_string(&mut self.gc, resource.clone()),
                );
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                "ResourceConflict"
            }
            MC::Events {
                detail,
                base,
                ours,
                theirs,
            } => {
                fields.insert(
                    "detail".into(),
                    Value::from_string(&mut self.gc, detail.clone()),
                );
                fields.insert("base".into(), Value::from_int(&mut self.gc, *base as i64));
                fields.insert("ours".into(), Value::from_int(&mut self.gc, *ours as i64));
                fields.insert(
                    "theirs".into(),
                    Value::from_int(&mut self.gc, *theirs as i64),
                );
                "EventConflict"
            }
        };
        Value::sum_type(
            &mut self.gc,
            "Conflict".to_string(),
            variant.to_string(),
            fields,
        )
    }

    fn bi_clock(&self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!("clock() takes no arguments, got {}", args.len()));
        }
        // SystemTime::now() traps on wasm32-unknown-unknown; the browser's
        // Date.now() is the wall clock there.
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Value::from_float(js_sys::Date::now() / 1000.0))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::SystemTime;
            let dur = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default();
            Ok(Value::from_float(dur.as_secs_f64()))
        }
    }

    fn bi_peek(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!("peek() expects 3 arguments, got {}", args.len()));
        }
        let snapshot = args[0]
            .as_world_fork()
            .ok_or_else(|| "peek() first argument must be a world_fork".to_string())?;
        let eid = args[1].as_entity_id().ok_or_else(|| {
            format!(
                "peek() second argument must be an entity, got {}",
                args[1].type_name()
            )
        })?;
        let ctype = Self::expect_component_type_name(&args[2], "peek")?;

        match snapshot.get_component(eid, &ctype) {
            Some(comp) => {
                let mut fields = std::collections::HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_component_data(&mut self.gc, comp),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                std::collections::HashMap::new(),
            )),
        }
    }

    /// `peek_resource(fork, Resource) -> Option<value>` — the resource
    /// dual of `peek`: read a fork's resource without committing.
    fn bi_peek_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "peek_resource() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let snapshot = args[0]
            .as_world_fork()
            .ok_or_else(|| "peek_resource() first argument must be a world_fork".to_string())?;
        let rtype = Self::expect_component_type_name(&args[1], "peek_resource")?;

        match snapshot.get_resource(&rtype) {
            Some(data) => {
                let mut fields = std::collections::HashMap::new();
                fields.insert(
                    "value".to_string(),
                    Value::from_component_data(&mut self.gc, data),
                );
                Ok(Value::sum_type(
                    &mut self.gc,
                    "Option".to_string(),
                    "Some".to_string(),
                    fields,
                ))
            }
            None => Ok(Value::sum_type(
                &mut self.gc,
                "Option".to_string(),
                "None".to_string(),
                std::collections::HashMap::new(),
            )),
        }
    }

    fn make_result(&mut self, ok: bool, value: Value) -> Value {
        let mut fields = std::collections::HashMap::new();
        // Language convention: `Ok { value }` but `Err { message }` â€” the
        // parser desugars `Err(x)` patterns to the `message` field.
        let field = if ok { "value" } else { "message" };
        fields.insert(field.to_string(), value);
        Value::sum_type(
            &mut self.gc,
            "Result".to_string(),
            if ok { "Ok" } else { "Err" }.to_string(),
            fields,
        )
    }

    fn schedule_from_value(value: &Value, fn_name: &str) -> Result<Vec<String>, String> {
        let list = value
            .as_list()
            .ok_or_else(|| format!("{}() schedule argument must be a list of systems", fn_name))?;
        list.iter()
            .map(|v| {
                v.as_system_ref().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "{}() schedule must be a list of `system` values, got {}",
                        fn_name,
                        v.type_name()
                    )
                })
            })
            .collect()
    }

    /// `simulate_par(fork, schedule, ticks, n_forks, seed) -> [world_fork]`
    ///
    /// Runs `n_forks` independent simulations of the same starting fork in
    /// parallel on the worker-VM pool. Each fork gets a deterministic RNG seed
    /// derived from `seed` and its index, so results are bit-identical for the
    /// same inputs regardless of thread count or scheduling order.
    fn bi_simulate_par(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 5 && args.len() != 6 {
            return Err(format!(
                "simulate_par() expects 5 arguments (plus an optional list of resource overrides), got {}",
                args.len()
            ));
        }
        let mut fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "simulate_par() first argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        // Optional 6th argument: resource overrides applied to the base fork
        // before any rollout runs — seed a candidate policy at the call site
        // instead of commit()ing it into the live world (dogfood feature seq
        // 150 #2). Same validation as `fork_with`, applied left to right.
        if let Some(overrides) = args.get(5) {
            let list = overrides.as_list().ok_or_else(|| {
                format!(
                    "simulate_par() sixth argument must be a list of resource values, got {}",
                    overrides.type_name()
                )
            })?;
            for (i, item) in list.iter().enumerate() {
                let data = item.as_component().ok_or_else(|| {
                    format!(
                        "simulate_par() override {} must be a resource value, got {}",
                        i,
                        item.type_name()
                    )
                })?;
                let name = data.type_name.clone();
                fork_snap = fork_snap.with_resource(&name, data.clone());
            }
        }
        let system_names = Self::schedule_from_value(&args[1], "simulate_par")?;
        let ticks = args[2].as_int().ok_or_else(|| {
            "simulate_par() third argument (ticks) must be an integer".to_string()
        })?;
        if ticks < 0 {
            return Err("simulate_par() tick count must be non-negative".to_string());
        }
        let n_forks = args[3].as_int().ok_or_else(|| {
            "simulate_par() fourth argument (n_forks) must be an integer".to_string()
        })?;
        if n_forks < 0 {
            return Err("simulate_par() fork count must be non-negative".to_string());
        }
        let seed = args[4]
            .as_int()
            .ok_or_else(|| "simulate_par() fifth argument (seed) must be an integer".to_string())?
            as u64;

        for name in &system_names {
            if !self.systems.contains_key(name) {
                return Err(format!("simulate_par(): unknown system '{}'", name));
            }
        }

        let shared = self.shared_state();
        let run_fork = |i: u64| {
            super::exec::with_worker_vm(&shared, |worker| {
                // Pending events are part of the forked state: each
                // worker timeline starts with the same in-flight queue.
                worker.restore_events_from(&fork_snap);
                worker.get_world_mut().restore(fork_snap.clone());
                worker.set_random_seed(crate::sandbox::fork_seed(seed, i));
                // The worker owns a private copy of the world, so ECS
                // writes apply directly instead of being deferred into the
                // command buffer (which would hide tick N's writes from
                // tick N+1).
                let was_worker = worker.is_worker;
                worker.is_worker = false;
                worker.in_simulation_fork += 1;

                let sim_result = (|| -> Result<(), String> {
                    for _ in 0..ticks {
                        for name in &system_names {
                            worker.run_system_by_name(name)?;
                        }
                        worker.bi_flush_events(vec![])?;
                    }
                    Ok(())
                })();

                worker.in_simulation_fork -= 1;
                worker.is_worker = was_worker;

                let snap = worker.snapshot_with_events();
                *worker.get_world_mut() = crate::world::World::new();
                worker.events_current.clear();
                worker.events_next.clear();
                worker.emit_ids_current.clear();
                worker.emit_ids_next.clear();
                // pooled workers must not carry timers into the next call
                worker.delayed_events.clear();
                sim_result.map(|_| snap)
            })
        };
        // wasm32 has no threads: the futures run sequentially on the same
        // pooled worker VM — identical results (each fork is seeded), no rayon.
        #[cfg(target_arch = "wasm32")]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> =
            (0..n_forks as u64).map(run_fork).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> = {
            use rayon::prelude::*;
            (0..n_forks as u64).into_par_iter().map(run_fork).collect()
        };

        let mut forks = Vec::with_capacity(snapshots.len());
        for (i, snap) in snapshots.into_iter().enumerate() {
            let mut snap = snap.map_err(|e| format!("simulate_par() fork {}: {}", i, e))?;
            // `fork_seed()` answers "which rng seed produced this rollout" —
            // hand that to `simulate_seeded` to reproduce it in isolation.
            snap.rollout_seed = Some(crate::sandbox::fork_seed(seed, i as u64));
            forks.push(Value::world_fork(&mut self.gc, std::sync::Arc::new(snap)));
        }
        Ok(Value::list(&mut self.gc, forks))
    }

    /// `simulate_many(forks, schedule, ticks, seed) -> [world_fork]`
    ///
    /// The heterogeneous sibling of `simulate_par`: instead of `n` rollouts of
    /// ONE fork, it runs each of the PROVIDED forks in parallel under the same
    /// schedule for `ticks`. This is the axis a search wants — B×K distinct
    /// candidate worlds evaluated at once — where `simulate_par`'s single-fork
    /// fan-out is the wrong dimension (dogfood feature seq 150). Per-fork RNG
    /// seeds derive from `(seed, index)` exactly like `simulate_par`, so a
    /// result is bit-identical for the same inputs regardless of thread count.
    fn bi_simulate_many(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "simulate_many() expects 4 arguments (forks, schedule, ticks, seed), got {}",
                args.len()
            ));
        }
        let bases: Vec<crate::world::WorldSnapshot> = {
            let list = args[0].as_list().ok_or_else(|| {
                "simulate_many() first argument must be a list of world_fork".to_string()
            })?;
            list.iter()
                .enumerate()
                .map(|(i, v)| {
                    v.as_world_fork()
                        .map(|f| f.as_ref().clone())
                        .ok_or_else(|| {
                            format!(
                                "simulate_many() element {} must be a world_fork, got {}",
                                i,
                                v.type_name()
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        let system_names = Self::schedule_from_value(&args[1], "simulate_many")?;
        let ticks = args[2].as_int().ok_or_else(|| {
            "simulate_many() third argument (ticks) must be an integer".to_string()
        })?;
        if ticks < 0 {
            return Err("simulate_many() tick count must be non-negative".to_string());
        }
        let seed = args[3].as_int().ok_or_else(|| {
            "simulate_many() fourth argument (seed) must be an integer".to_string()
        })? as u64;

        for name in &system_names {
            if !self.systems.contains_key(name) {
                return Err(format!("simulate_many(): unknown system '{}'", name));
            }
        }

        let shared = self.shared_state();
        let bases_ref = &bases;
        let run_fork = |i: usize| {
            super::exec::with_worker_vm(&shared, |worker| {
                let base = bases_ref[i].clone();
                worker.restore_events_from(&base);
                worker.get_world_mut().restore(base);
                worker.set_random_seed(crate::sandbox::fork_seed(seed, i as u64));
                let was_worker = worker.is_worker;
                worker.is_worker = false;
                worker.in_simulation_fork += 1;

                let sim_result = (|| -> Result<(), String> {
                    for _ in 0..ticks {
                        for name in &system_names {
                            worker.run_system_by_name(name)?;
                        }
                        worker.bi_flush_events(vec![])?;
                    }
                    Ok(())
                })();

                worker.in_simulation_fork -= 1;
                worker.is_worker = was_worker;

                let snap = worker.snapshot_with_events();
                *worker.get_world_mut() = crate::world::World::new();
                worker.events_current.clear();
                worker.events_next.clear();
                worker.emit_ids_current.clear();
                worker.emit_ids_next.clear();
                worker.delayed_events.clear();
                sim_result.map(|_| snap)
            })
        };
        #[cfg(target_arch = "wasm32")]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> =
            (0..bases.len()).map(run_fork).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let snapshots: Vec<Result<crate::world::WorldSnapshot, String>> = {
            use rayon::prelude::*;
            (0..bases.len()).into_par_iter().map(run_fork).collect()
        };

        let mut forks = Vec::with_capacity(snapshots.len());
        for (i, snap) in snapshots.into_iter().enumerate() {
            let mut snap = snap.map_err(|e| format!("simulate_many() fork {}: {}", i, e))?;
            snap.rollout_seed = Some(crate::sandbox::fork_seed(seed, i as u64));
            forks.push(Value::world_fork(&mut self.gc, std::sync::Arc::new(snap)));
        }
        Ok(Value::list(&mut self.gc, forks))
    }

    /// `fork_with(fork, resource_value) -> world_fork`
    ///
    /// Returns a copy of `fork` with one resource overridden — a speculative
    /// candidate seeded WITHOUT `commit()`ing to the live world (dogfood
    /// feature seq 150). `resource_value` is a resource/component instance
    /// (e.g. `Policy { tax: 8 }`); its type name selects the resource. Events,
    /// timers, and entities ride through untouched, so the result composes
    /// straight into `simulate`/`simulate_par`/`simulate_many`.
    fn bi_fork_with(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "fork_with() expects 2 arguments (fork, resource value), got {}",
                args.len()
            ));
        }
        let base = args[0]
            .as_world_fork()
            .ok_or_else(|| "fork_with() first argument must be a world_fork".to_string())?;
        let data = args[1].as_component().ok_or_else(|| {
            format!(
                "fork_with() second argument must be a component/resource value, got {}",
                args[1].type_name()
            )
        })?;
        let name = data.type_name.clone();
        let new_snap = base.as_ref().with_resource(&name, data.clone());
        Ok(Value::world_fork(
            &mut self.gc,
            std::sync::Arc::new(new_snap),
        ))
    }

    /// `simulate_seeded(fork, schedule, ticks, raw_seed) -> world_fork`
    ///
    /// ONE rollout under an EXACT rng seed — no per-index derivation. This is
    /// the reproduction half of `fork_seed()` (dogfood feature seq 150): when
    /// rollout `i` of a `simulate_par`/`simulate_many` call is the outlier,
    /// `simulate_seeded(base, systems, ticks, fork_seed(outs[i]))` re-runs
    /// exactly that future in isolation, bit-identically, without paying for
    /// the other rollouts. Purity rules match `simulate_par` (rand_* allowed —
    /// the explicit seed keeps it deterministic).
    fn bi_simulate_seeded(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(format!(
                "simulate_seeded() expects 4 arguments (fork, systems, ticks, raw_seed), got {}",
                args.len()
            ));
        }
        let fork_snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "simulate_seeded() first argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        let system_names = Self::schedule_from_value(&args[1], "simulate_seeded")?;
        let ticks = args[2].as_int().ok_or_else(|| {
            "simulate_seeded() third argument (ticks) must be an integer".to_string()
        })?;
        if ticks < 0 {
            return Err("simulate_seeded() tick count must be non-negative".to_string());
        }
        let raw_seed = args[3].as_int().ok_or_else(|| {
            "simulate_seeded() fourth argument (raw_seed) must be an integer".to_string()
        })? as u64;

        for name in &system_names {
            if !self.systems.contains_key(name) {
                return Err(format!("simulate_seeded(): unknown system '{}'", name));
            }
        }

        let shared = self.shared_state();
        // Run the one rollout on a rayon POOL thread, never the caller (a
        // one-item par_iter would execute on the calling thread): the pooled
        // worker VM must live in a pool thread's TLS, which outlives any
        // caller. Parking a worker VM in a short-lived caller thread's TLS
        // (e.g. a test thread) wedges that thread's teardown when the TLS
        // destructor tears the whole VM down.
        let run_rollout = move || {
            super::exec::with_worker_vm(&shared, |worker| {
                worker.restore_events_from(&fork_snap);
                worker.get_world_mut().restore(fork_snap.clone());
                // The seed is used AS GIVEN — that is the whole point.
                worker.set_random_seed(raw_seed);
                let was_worker = worker.is_worker;
                worker.is_worker = false;
                worker.in_simulation_fork += 1;

                let sim_result = (|| -> Result<(), String> {
                    for _ in 0..ticks {
                        for name in &system_names {
                            worker.run_system_by_name(name)?;
                        }
                        worker.bi_flush_events(vec![])?;
                    }
                    Ok(())
                })();

                worker.in_simulation_fork -= 1;
                worker.is_worker = was_worker;

                let snap = worker.snapshot_with_events();
                *worker.get_world_mut() = crate::world::World::new();
                worker.events_current.clear();
                worker.events_next.clear();
                worker.emit_ids_current.clear();
                worker.emit_ids_next.clear();
                worker.delayed_events.clear();
                sim_result.map(|_| snap)
            })
        };
        // wasm32 has no threads: run on the (only) thread, like simulate_par.
        #[cfg(target_arch = "wasm32")]
        let snap = run_rollout();
        #[cfg(not(target_arch = "wasm32"))]
        let snap = {
            let (tx, rx) = std::sync::mpsc::channel();
            rayon::spawn(move || {
                let _ = tx.send(run_rollout());
            });
            rx.recv()
                .map_err(|_| "simulate_seeded(): rollout worker disappeared".to_string())?
        };
        let mut snap = snap.map_err(|e| format!("simulate_seeded(): {}", e))?;
        snap.rollout_seed = Some(raw_seed);
        Ok(Value::world_fork(&mut self.gc, std::sync::Arc::new(snap)))
    }

    /// `fork_seed(fork) -> int`
    ///
    /// The effective rng seed the simulate-family rollout that produced this
    /// fork ran under, or 0 for any other fork (`fork()`, `fork_with`,
    /// `merge_forks`, wire decodes — the seed is local debug metadata and is
    /// deliberately not serialized). Derived seeds are never 0 (the SplitMix64
    /// finalizer clamps 0 to a sentinel), so 0 is unambiguous.
    fn bi_fork_seed(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "fork_seed() expects 1 argument, got {}",
                args.len()
            ));
        }
        let fork = args[0]
            .as_world_fork()
            .ok_or_else(|| "fork_seed() argument must be a world_fork".to_string())?;
        let seed = fork.as_ref().rollout_seed.unwrap_or(0);
        Ok(Value::from_int(&mut self.gc, seed as i64))
    }

    /// Compile untrusted RAD source for sandboxed execution.
    ///
    /// Module imports are rejected outright: resolving them would touch the
    /// filesystem, which a sandboxed guest must never do.
    fn compile_sandbox_source(source: &str) -> Result<crate::compiler::CompileResult, String> {
        let mut lexer = crate::lexer::Lexer::new(source);
        let (tokens, lex_errors) = lexer.tokenize();

        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse();

        let mut all_errors = Vec::new();
        for e in lex_errors {
            all_errors.push(format!(
                "[line {}:{}] Lex error: {}",
                e.line, e.col, e.message
            ));
        }
        for e in parser.errors() {
            all_errors.push(format!(
                "[line {}:{}] Parse error: {}",
                e.line, e.col, e.message
            ));
        }
        if program
            .declarations
            .iter()
            .any(|d| matches!(d, crate::ast::Decl::Use(_)))
        {
            all_errors
                .push("sandbox: module imports are not permitted in sandboxed code".to_string());
        }

        let mut checker = crate::checker::Checker::new();
        let checker_errors = checker.check(&program);
        let checker_output = checker.output();
        for e in checker_errors {
            all_errors.push(format!(
                "[line {}:{}] Type error: {}",
                e.line, e.col, e.message
            ));
        }
        if !all_errors.is_empty() {
            return Err(all_errors.join("\n"));
        }

        crate::compiler::Compiler::new()
            .with_checker_output(checker_output)
            .compile(&program)
            .map_err(|e| format!("Compile error: {}", e.message))
    }

    /// `sandbox_run(source, fork, caps_json) -> Result`
    ///
    /// Compiles and runs untrusted RAD source against a forked world inside a
    /// fresh, capability-bounded guest VM. The guest never sees the live
    /// world; it gets a copy-on-write fork and must return it for the host to
    /// inspect (`peek`/`diff`) and optionally `commit`.
    ///
    /// Enforcement layers:
    /// 1. builtin mask â€” IO/network/clock/speculation builtins are denied
    ///    (`call_builtin` checks `sandbox_caps`),
    /// 2. component-write ACL â€” `set`/`spawn`/`despawn`/system writebacks are
    ///    checked against the caps allowlist,
    /// 3. budgets â€” fuel and memory limits bound all execution.
    ///
    /// Unlike `simulate()`, events emitted by the guest are *not* dropped:
    /// the guest VM owns private double-buffered queues, so its handlers run
    /// normally inside the closed world (captured-events mode).
    ///
    /// Failure of any kind — malformed capability grant, guest compile
    /// error, runtime error, budget exhaustion, capability denial — returns
    /// `Err(message)` rather than aborting the host.
    fn bi_sandbox_run(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 && args.len() != 4 {
            return Err(format!(
                "sandbox_run() expects 3 or 4 arguments, got {}",
                args.len()
            ));
        }
        let source = args[0]
            .as_str()
            .ok_or_else(|| "sandbox_run() first argument must be a source string".to_string())?
            .to_string();
        let fork_snap = args[1]
            .as_world_fork()
            .ok_or_else(|| "sandbox_run() second argument must be a world_fork".to_string())?
            .as_ref()
            .clone();
        let caps_text = args[2]
            .as_str()
            .ok_or_else(|| "sandbox_run() third argument must be a caps JSON string".to_string())?;
        // A malformed grant comes back through the Err arm like every other
        // sandbox_run failure (dogfood bug seq 59). "Malformed caps are a
        // host-side bug: hard error" held only while caps were literals in
        // host source; a real plugin host computes the grant from the
        // plugin's own manifest, so a bad manifest ("fuel": -1) is
        // attacker-influenced input and must not take the host down before
        // the ACL or budgets ever run. The parser's messages stay strict and
        // specific — only the failure mode changes.
        let (caps, seed) = match crate::sandbox::SandboxCaps::from_json(caps_text) {
            Ok(parsed) => parsed,
            Err(e) => {
                let v = Value::from_string(&mut self.gc, e);
                return Ok(self.make_result(false, v));
            }
        };

        // Optional 4th argument: data-only input for the guest's
        // sandbox_input(). Serialized to JSON immediately — values are parsed
        // onto the guest heap, never shared. Replaces the anti-pattern of
        // splicing host data into guest source text.
        let input_json = match args.get(3) {
            Some(v) => Some(
                value_to_json(v, 0)
                    .map_err(|e| format!("sandbox_run() input is not data-only: {}", e))?
                    .to_string(),
            ),
            None => None,
        };

        let outcome = Self::run_sandbox_guest(
            &source,
            fork_snap,
            caps,
            seed,
            input_json,
            self.component_field_types.clone(),
        );

        // Retain the guest's structured output and fuel spend for
        // `sandbox_last_output()` / `sandbox_last_fuel()` (dogfood feature
        // seq 62): both were computed and discarded before, leaving a Rad
        // host with only Result<world_fork, str>. Recorded on every run,
        // including failures — a partial run still spends fuel, and the fuel
        // number is exactly the telemetry a plugin host meters on.
        self.last_sandbox_output_json = outcome.output_json.clone();
        self.last_sandbox_fuel_spent = outcome.fuel_spent;

        // Surface buffered guest output to the host, tagged for provenance.
        for line in outcome.prints {
            let tagged = format!("[sandbox] {}", line);
            if self.suppress_output {
                self.print_buffer.push(tagged);
            } else {
                println!("{}", tagged);
            }
        }

        match outcome.result {
            Ok(snap) => {
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(e) => {
                let v = Value::from_string(&mut self.gc, e);
                Ok(self.make_result(false, v))
            }
        }
    }

    /// Run untrusted source in a capability-bounded guest VM against a world
    /// snapshot. This is the shared core behind the `sandbox_run` builtin and
    /// the `rad sandbox serve` JSON-RPC protocol.
    ///
    /// `input_json` crosses the data-only boundary: the guest reads it via
    /// `sandbox_input()` (parsed onto the guest's own heap) and reports
    /// structured results via `sandbox_output(v)` (serialized to JSON before
    /// the guest VM is dropped). No heap values ever cross between VMs.
    pub fn run_sandbox_guest(
        source: &str,
        base: crate::world::WorldSnapshot,
        caps: crate::sandbox::SandboxCaps,
        seed: u64,
        input_json: Option<String>,
        host_field_types: crate::vm::ComponentFieldTypes,
    ) -> crate::sandbox::SandboxOutcome {
        let initial_fuel = caps.fuel;
        let compile_result = match Self::compile_sandbox_source(source) {
            Ok(r) => r,
            Err(msg) => {
                return crate::sandbox::SandboxOutcome {
                    result: Err(msg),
                    prints: Vec::new(),
                    fuel_spent: 0,
                    output_json: None,
                }
            }
        };

        let mem_limit = caps.mem_limit;
        let mut guest = VM::new();
        guest.suppress_output();
        guest.load_compile_result(compile_result);
        // The write-shape ACL binds guest writes to the HOST's declared
        // schema, so the guest must be validated against the host's field
        // types, not its own (a malicious guest declares the wrong shape on
        // purpose). Overlay the host schema onto the guest's: host-declared
        // components become authoritative; guest-only components keep theirs.
        if !host_field_types.is_empty() {
            let ft = std::sync::Arc::make_mut(&mut guest.component_field_types);
            for (name, fields) in host_field_types.iter() {
                ft.insert(name.clone(), fields.clone());
            }
        }
        // The fork's pending events are part of the state handed to the
        // guest. `guest.run` resets the queues, so they are spliced in after
        // the main chunk, ahead of the guest's own emissions (FIFO from the
        // forked timeline), and drained below.
        let inherited_events: Vec<(String, Value, u64)> = base.events.as_ref().clone();
        let inherited_emit_ids: Vec<u64> = base.emit_ids.as_ref().clone();
        guest.get_world_mut().restore(base);
        guest.sandbox_caps = Some(std::sync::Arc::new(caps));
        guest.fuel = initial_fuel;
        guest.mem_limit = mem_limit;
        guest.set_random_seed(seed);
        guest.sandbox_input_json = input_json;

        let run_result = guest.run(0).and_then(|_| {
            if !inherited_events.is_empty() {
                let mut q = inherited_events;
                q.extend(std::mem::take(&mut guest.events_next));
                guest.events_next = q;
                let mut ids = inherited_emit_ids;
                ids.extend(std::mem::take(&mut guest.emit_ids_next));
                guest.emit_ids_next = ids;
            }
            // Drain any events still in flight after the guest's main chunk so
            // emitted events take effect inside the closed world.
            //
            // Every generation is charged. This loop is Rust, not bytecode, and
            // a handler body carrying no call and no loop back-edge crosses no
            // charge point of its own (`emit` is a single opcode), so a
            // self-re-emitting handler would otherwise drain forever — unmetered
            // by fuel, and unmetered by `mem_bytes` too, since the allocation
            // ceiling is only enforced inside `charge_fuel`.
            while !guest.events_next.is_empty() {
                guest.charge_fuel()?;
                guest.bi_flush_events(vec![])?;
            }
            Ok(())
        });

        crate::sandbox::SandboxOutcome {
            result: run_result.map(|_| guest.snapshot_with_events()),
            prints: std::mem::take(&mut guest.print_buffer),
            fuel_spent: initial_fuel.saturating_sub(guest.fuel),
            output_json: guest.sandbox_output_json.take(),
        }
    }

    /// `fork_to_bytes(fork) -> str` â€” the fork wire codec, encode half.
    /// Serializes a fork's **full program state** â€” entities (with their
    /// runtime ids and the id-allocator), names, components, resources,
    /// in-flight events with causality ids, and the schema â€” as canonical
    /// JSON with an integrity digest. Deterministic: the same fork encodes
    /// to the same bytes on every machine.
    ///
    /// Format: `RADFORK2 <blake3-hex> <body-json>`, written directly into a
    /// string buffer (no intermediate JSON tree) with the compact value
    /// codec in `crate::wire` â€” measured ~30x faster and ~4x smaller than
    /// the v1 tagged-tree format.
    pub(crate) fn bi_fork_to_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        use std::fmt::Write as _;
        if args.len() != 1 {
            return Err(format!(
                "fork_to_bytes() expects 1 argument, got {}",
                args.len()
            ));
        }
        let snap = args[0]
            .as_world_fork()
            .ok_or_else(|| "fork_to_bytes() argument must be a world_fork".to_string())?
            .as_ref()
            .clone();

        let mut w = crate::world::World::new();
        let events = snap.events.clone();
        let emit_ids = snap.emit_ids.clone();
        let delayed = snap.delayed.clone();
        let provenance = snap.provenance.clone();
        let next_id = snap.next_id;
        let mut free_ids = snap.free_ids.clone();
        free_ids.sort_unstable();
        w.restore(snap);

        // First occurrence of a type pins its wire layout; later instances
        // (which can only differ in order, not field set) remap into it.
        let mut schema: std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>> =
            std::collections::BTreeMap::new();
        fn write_data(
            schema: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>>,
            data: &crate::value::ComponentData,
            out: &mut String,
        ) -> Result<(), String> {
            let wire_layout = schema
                .entry(data.type_name.clone())
                .or_insert_with(|| data.layout.clone())
                .clone();
            crate::wire::escape_json_into(out, &data.type_name);
            out.push_str(",[");
            let aligned =
                std::sync::Arc::ptr_eq(&wire_layout, &data.layout) || *wire_layout == *data.layout;
            for i in 0..wire_layout.len() {
                if i > 0 {
                    out.push(',');
                }
                let v = if aligned {
                    &data.values[i]
                } else {
                    let f = &wire_layout[i];
                    let pos = data.layout.iter().position(|n| n == f).ok_or_else(|| {
                        format!(
                            "fork_to_bytes: instances of '{}' disagree on field '{}'",
                            data.type_name, f
                        )
                    })?;
                    &data.values[pos]
                };
                crate::wire::encode_value_into(v, out)?;
            }
            out.push(']');
            Ok(())
        }

        let mut body = String::with_capacity(64 * 1024);
        body.push_str("{\"entities\":[");
        for (i, eid) in w.all_entity_ids().into_iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "[{},", eid);
            match w.entity_name(eid) {
                Some(name) => crate::wire::escape_json_into(&mut body, &name),
                None => body.push_str("null"),
            }
            body.push_str(",[");
            let mut comps = w.components_on_entity(eid);
            comps.sort_by(|a, b| a.type_name.cmp(&b.type_name));
            for (j, data) in comps.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                write_data(&mut schema, data, &mut body)?;
                body.push(']');
            }
            body.push_str("]]");
        }

        body.push_str("],\"events\":[");
        for (i, (name, payload, tid)) in events.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, name);
            // Emit ids cross into the foreign namespace here: the receiver's
            // ledger must never confuse them with its own sequential ids.
            let _ = write!(
                body,
                ",{},{},",
                tid,
                crate::causality::foreign_emit_id(emit_ids.get(i).copied().unwrap_or(0))
            );
            crate::wire::encode_value_into(payload, &mut body)?;
            body.push(']');
        }

        body.push_str("],\"delayed\":[");
        for (i, (left, name, payload, emit_id)) in delayed.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            let _ = write!(body, "{},", left);
            crate::wire::escape_json_into(&mut body, name);
            let _ = write!(body, ",{},", crate::causality::foreign_emit_id(*emit_id));
            crate::wire::encode_value_into(payload, &mut body)?;
            body.push(']');
        }

        body.push_str("],\"free_ids\":[");
        for (i, fid) in free_ids.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "{}", fid);
        }
        let _ = write!(body, "],\"next_id\":{},\"resources\":[", next_id);

        let mut rnames = w.resource_names();
        rnames.sort();
        for (i, rname) in rnames.iter().enumerate() {
            if let Some(data) = w.get_resource(rname) {
                if i > 0 {
                    body.push(',');
                }
                body.push('[');
                write_data(&mut schema, &data, &mut body)?;
                body.push(']');
            }
        }

        body.push_str("],\"schema\":[");
        for (i, (tname, layout)) in schema.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, tname);
            body.push_str(",[");
            for (j, f) in layout.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, f);
            }
            body.push_str("]]");
        }

        // Provenance rides last (tools that only want state can stop at it).
        // A decoded fork re-encodes its carried records verbatim — that is
        // what keeps re-encoding byte-identical across machines. A local
        // fork ships this VM's ledger closure for everything alive in it.
        body.push_str("],\"prov\":");
        match &provenance {
            Some(p) => crate::wire::encode_prov_into(p, &mut body),
            None => {
                let resource_names: std::collections::HashSet<String> =
                    w.resource_names().into_iter().collect();
                let closure = self.ledger.provenance_closure(
                    |rec| match rec.entity {
                        Some(eid) => w.contains_entity(eid),
                        None => resource_names.contains(&rec.component),
                    },
                    &emit_ids
                        .iter()
                        .copied()
                        .chain(delayed.iter().map(|(_, _, _, id)| *id))
                        .collect::<Vec<_>>(),
                );
                crate::wire::encode_prov_into(&closure, &mut body);
            }
        }
        body.push('}');

        // RADPACK (D1): big bodies ship compressed, small ones stay legacy
        // JSON; the digest names the same world either way.
        let out = crate::radpack::seal("RADFORK2", &body);
        Ok(Value::from_string(&mut self.gc, out))
    }

    /// `fork_from_bytes(str) -> Result<world_fork, str>` â€” decode half.
    /// Verifies the integrity digest, reconstructs the world **id-faithfully**
    /// (entity ids, names, allocator state), revives in-flight events, and
    /// validates every component/resource against the local schema â€” running
    /// declared `migrate` blocks on shape drift, exactly like `load_world`.
    /// Malformed or mismatched bytes are an `Err`, not a crash: network input
    /// is a system boundary.
    pub(crate) fn bi_fork_from_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "fork_from_bytes() expects 1 argument, got {}",
                args.len()
            ));
        }
        let text = args[0]
            .as_str()
            .ok_or_else(|| format!("fork_from_bytes() expects str, got {}", args[0].type_name()))?
            .to_string();

        match self.decode_fork_wire(&text) {
            Ok(snap) => {
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(msg) => {
                let e = Value::from_string(&mut self.gc, msg);
                Ok(self.make_result(false, e))
            }
        }
    }

    fn decode_fork_wire(&mut self, text: &str) -> Result<crate::world::WorldSnapshot, String> {
        // RADPACK envelopes are opened (inflate + digest check) first; the
        // legacy parser below then runs unchanged on either vintage.
        let text = crate::radpack::open(text).map_err(|e| format!("fork_from_bytes: {}", e))?;
        let text: &str = &text;
        // Header: `RADFORK2 <blake3-hex> <body>` â€” the digest is verified
        // against the raw body bytes before any parsing.
        let rest = text
            .strip_prefix("RADFORK2 ")
            .ok_or("fork_from_bytes: not a rad-fork payload (expected RADFORK2 header)")?;
        let (claimed, body_text) = rest
            .split_once(' ')
            .ok_or("fork_from_bytes: malformed header")?;
        let actual = blake3::hash(body_text.as_bytes()).to_hex();
        if claimed != actual.as_str() {
            return Err(format!(
                "fork_from_bytes: integrity digest mismatch (claimed {}â€¦, computed {}â€¦) â€” \
                 payload corrupted or tampered",
                crate::radpack::preview(claimed, 12),
                &actual.as_str()[..12]
            ));
        }
        let body: serde_json::Value = serde_json::from_str(body_text)
            .map_err(|e| format!("fork_from_bytes: invalid JSON: {}", e))?;

        // Schema: wire layout per type, [[name, [fields]], ...].
        let mut schema: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for entry in body["schema"].as_array().into_iter().flatten() {
            let pair = entry.as_array().filter(|a| a.len() == 2);
            let (Some(pair),) = (pair,) else {
                return Err("fork_from_bytes: malformed schema entry".into());
            };
            let tname = pair[0]
                .as_str()
                .ok_or("fork_from_bytes: malformed schema entry")?;
            let fields: Vec<String> = pair[1]
                .as_array()
                .ok_or("fork_from_bytes: malformed schema entry")?
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            schema.insert(tname.to_string(), fields);
        }

        // Realize plan, computed once per type instead of once per instance:
        // identical field sets decode straight into declared order; drift
        // goes through the declared `migrate` block per instance.
        enum Plan {
            Direct {
                declared: std::sync::Arc<Vec<String>>,
                // declared index -> stored index
                map: Vec<usize>,
            },
            Migrate {
                stored: Vec<String>,
                declared: std::sync::Arc<Vec<String>>,
            },
        }
        let make_plan = |stored: &[String], declared: std::sync::Arc<Vec<String>>| -> Plan {
            if stored.len() == declared.len() {
                let mut map = Vec::with_capacity(declared.len());
                for f in declared.iter() {
                    match stored.iter().position(|s| s == f) {
                        Some(i) => map.push(i),
                        None => {
                            return Plan::Migrate {
                                stored: stored.to_vec(),
                                declared,
                            }
                        }
                    }
                }
                Plan::Direct { declared, map }
            } else {
                Plan::Migrate {
                    stored: stored.to_vec(),
                    declared,
                }
            }
        };
        let mut plans: std::collections::HashMap<String, Plan> = std::collections::HashMap::new();

        // Allocator first, entities second. Every consistent world satisfies
        // next_id == live + free (every issued id is one or the other), so a
        // payload whose next_id exceeds what its own tables account for is
        // malformed — and validating that *before* inserting is what keeps a
        // hostile id (fuzzer finding: a single entity with id 2^64-1) from
        // flooding the free-list gap-fill for 60 seconds and then aborting
        // on u32 overflow.
        let ents_len = body["entities"].as_array().map_or(0, |a| a.len()) as u64;
        let next_id_u64 = body["next_id"]
            .as_u64()
            .ok_or("fork_from_bytes: missing next_id")?;
        let mut free_ids: Vec<u32> = Vec::new();
        for v in body["free_ids"].as_array().into_iter().flatten() {
            let id = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("fork_from_bytes: free id out of range")?;
            free_ids.push(id);
        }
        if next_id_u64 > ents_len + free_ids.len() as u64 {
            return Err(format!(
                "fork_from_bytes: allocator claims {} ids issued but the payload \
                 accounts for only {} ({} live + {} free)",
                next_id_u64,
                ents_len + free_ids.len() as u64,
                ents_len,
                free_ids.len()
            ));
        }
        let next_id = u32::try_from(next_id_u64)
            .map_err(|_| "fork_from_bytes: next_id out of range".to_string())?;

        let mut w = crate::world::World::new();
        // Seed the program's `indexed` declarations BEFORE inserting rows:
        // the bulk insert maintains indices as it goes, so a snapshot that
        // crossed a wire carries working indexes instead of wiping the live
        // world's on commit (Tier-1 finding: every RADTRACK client lost its
        // indexes the moment it pulled).
        w.share_indexed_fields_from(&self.world);

        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ent in body["entities"].as_array().into_iter().flatten() {
            let parts = ent
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("fork_from_bytes: malformed entity entry")?;
            let eid_u64 = parts[0]
                .as_u64()
                .ok_or("fork_from_bytes: entity without id")?;
            let eid = u32::try_from(eid_u64)
                .ok()
                .filter(|&id| id < next_id)
                .ok_or_else(|| {
                    format!(
                        "fork_from_bytes: entity id {} is outside the allocator \
                         range (next_id {})",
                        eid_u64, next_id
                    )
                })?;
            let name = parts[1].as_str();
            Self::validate_loaded_entity_name("fork_from_bytes", name, &mut seen_names)?;
            let comps_json = parts[2]
                .as_array()
                .ok_or("fork_from_bytes: malformed entity components")?;

            let mut comps: Vec<crate::value::ComponentData> = Vec::with_capacity(comps_json.len());
            for centry in comps_json {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_from_bytes: malformed component entry")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("fork_from_bytes: malformed component entry")?;
                let vals = cpair[1]
                    .as_array()
                    .ok_or("fork_from_bytes: malformed component values")?;

                if !plans.contains_key(cname) {
                    let stored = schema.get(cname).ok_or_else(|| {
                        format!("fork_from_bytes: no schema entry for '{}'", cname)
                    })?;
                    let declared = self.component_layouts.get(cname).cloned().ok_or_else(|| {
                        format!(
                            "fork_from_bytes: payload contains component '{}' which is \
                                 not declared in this program",
                            cname
                        )
                    })?;
                    plans.insert(cname.to_string(), make_plan(stored, declared));
                }
                let data = match plans.get(cname).unwrap() {
                    Plan::Direct { declared, map } => {
                        if vals.len() != map.len() {
                            return Err(format!(
                                "fork_from_bytes: '{}' row has {} values, schema says {}",
                                cname,
                                vals.len(),
                                map.len()
                            ));
                        }
                        // Decode into the gc heap; the bulk insert persists
                        // rows exactly once (persisting twice would leak the
                        // manually ref-counted persistent objects).
                        let mut values = Vec::with_capacity(map.len());
                        for &si in map {
                            values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                        }
                        let data = crate::value::ComponentData {
                            type_name: cname.to_string(),
                            layout: declared.clone(),
                            values,
                        };
                        self.validate_loaded_row("fork_from_bytes", &data)?;
                        data
                    }
                    Plan::Migrate { stored, declared } => {
                        let (stored, declared) = (stored.clone(), declared.clone());
                        self.migrate_wire_row(cname, &stored, declared, vals, 0)?
                    }
                };
                comps.push(data);
            }
            if !w.insert_entity_with_components(eid, name, comps) {
                return Err(format!("fork_from_bytes: duplicate entity id {}", eid));
            }
        }

        for rentry in body["resources"].as_array().into_iter().flatten() {
            let rpair = rentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_from_bytes: malformed resource entry")?;
            let rname = rpair[0]
                .as_str()
                .ok_or("fork_from_bytes: malformed resource entry")?;
            let vals = rpair[1]
                .as_array()
                .ok_or("fork_from_bytes: malformed resource values")?;
            let stored = schema
                .get(rname)
                .ok_or_else(|| format!("fork_from_bytes: no schema entry for '{}'", rname))?;
            let declared = self
                .world
                .get_resource(rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "fork_from_bytes: payload contains resource '{}' which is not \
                         declared in this program",
                        rname
                    )
                })?;
            let data = match make_plan(stored, declared) {
                Plan::Direct { declared, map } => {
                    if vals.len() != map.len() {
                        return Err(format!(
                            "fork_from_bytes: resource '{}' has {} values, schema says {}",
                            rname,
                            vals.len(),
                            map.len()
                        ));
                    }
                    let mut values = Vec::with_capacity(map.len());
                    for si in map {
                        values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                    }
                    let data = crate::value::ComponentData {
                        type_name: rname.to_string(),
                        layout: declared,
                        values,
                    };
                    self.validate_loaded_row("fork_from_bytes", &data)?;
                    data
                }
                Plan::Migrate { stored, declared } => {
                    self.migrate_wire_row(rname, &stored, declared, vals, 0)?
                }
            };
            // set_resource persists; persisting here too would abandon a
            // full copy per ingested resource (leak-lab finding).
            w.set_resource(rname, data);
        }

        w.set_id_allocator(next_id, free_ids)
            .map_err(|e| format!("fork_from_bytes: {}", e))?;

        let mut events: Vec<(String, Value, u64)> = Vec::new();
        let mut emit_ids: Vec<u64> = Vec::new();
        for ev in body["events"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or("fork_from_bytes: malformed event entry")?;
            let name = parts[0]
                .as_str()
                .ok_or("fork_from_bytes: event without name")?
                .to_string();
            let tid = parts[1].as_u64().unwrap_or(0);
            let emit_id = parts[2].as_u64().unwrap_or(0);
            // Event payloads live in the snapshot itself: decode straight
            // into the persistent store (nothing persists them later).
            let payload = crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[3])?;
            events.push((name, payload, tid));
            emit_ids.push(emit_id);
        }

        // delayed timers: optional section, absent in pre-delayed tapes
        let mut delayed: Vec<(i64, String, Value, u64)> = Vec::new();
        for ev in body["delayed"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 3 || a.len() == 4)
                .ok_or("fork_from_bytes: malformed delayed entry")?;
            let left = parts[0]
                .as_i64()
                .ok_or("fork_from_bytes: delayed entry without tick count")?;
            let name = parts[1]
                .as_str()
                .ok_or("fork_from_bytes: delayed entry without name")?
                .to_string();
            let (emit_id, payload_idx) = if parts.len() == 4 {
                (parts[2].as_u64().unwrap_or(0), 3)
            } else {
                (0, 2)
            };
            let payload =
                crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[payload_idx])?;
            delayed.push((left, name, payload, emit_id));
        }

        let mut snap = w.snapshot();
        snap.events = std::sync::Arc::new(events);
        snap.emit_ids = std::sync::Arc::new(emit_ids);
        snap.delayed = std::sync::Arc::new(delayed);
        // The sender's provenance closure rides along; commit() ingests it
        // into the local ledger so why() can answer across the seam. The
        // origin label is the payload digest — the receiver names what it
        // can verify, not what the sender claims.
        if let Some(pj) = body.get("prov") {
            let mut prov = crate::wire::decode_prov(pj)?;
            prov.origin = format!("wire {}", crate::radpack::preview(claimed, 8));
            snap.provenance = Some(std::sync::Arc::new(prov));
        }
        Ok(snap)
    }

    /// Wire decode for a row whose schema drifted: decode the stored fields
    /// (into the gc heap â€” migration runs user code) and feed them through
    /// the declared `migrate` block, like `load_world` does. `from_version`
    /// is the save's declared schema version for this type (dogfood seq 69)
    /// — 0 for wire payloads and versionless saves.
    fn migrate_wire_row(
        &mut self,
        tname: &str,
        stored: &[String],
        declared: std::sync::Arc<Vec<String>>,
        vals: &[serde_json::Value],
        from_version: i64,
    ) -> Result<crate::value::ComponentData, String> {
        if vals.len() != stored.len() {
            return Err(format!(
                "fork_from_bytes: '{}' row has {} values, schema says {}",
                tname,
                vals.len(),
                stored.len()
            ));
        }
        let mut pairs = Vec::with_capacity(stored.len());
        for (f, j) in stored.iter().zip(vals) {
            pairs.push((f.clone(), crate::wire::decode_value(&mut self.gc, j)?));
        }
        self.migrate_loaded(tname, pairs, &declared, from_version)
    }

    /// `fork_delta(base, fork) -> str` — delta sync, encode half. Ships only
    /// the **divergence** of `fork` relative to `base`: upserted entities
    /// (full rows), despawns, changed resources, the in-flight queue, the
    /// allocator, the schema of shipped types, and the provenance closure
    /// **restricted to touched values** — delta sync pays double, shrinking
    /// state and history at once. The receiver reconstructs the fork with
    /// `fork_apply(its_own_base, delta)`; both sides must hold the same base
    /// (the payload carries a fingerprint, the protocol carries identity).
    pub(crate) fn bi_fork_delta(&mut self, args: Vec<Value>) -> Result<Value, String> {
        use std::fmt::Write as _;
        if args.len() != 2 {
            return Err(format!(
                "fork_delta() expects 2 arguments (base, fork), got {}",
                args.len()
            ));
        }
        let base = args[0]
            .as_world_fork()
            .cloned()
            .ok_or_else(|| "fork_delta() first argument must be a world_fork".to_string())?;
        let fork = args[1]
            .as_world_fork()
            .cloned()
            .ok_or_else(|| "fork_delta() second argument must be a world_fork".to_string())?;

        let mut wb = crate::world::World::new();
        wb.restore((*base).clone());
        let mut wf = crate::world::World::new();
        wf.restore((*fork).clone());

        let sorted_comps = |w: &crate::world::World, eid: u32| {
            let mut c = w.components_on_entity(eid);
            c.sort_by(|a, b| a.type_name.cmp(&b.type_name));
            c
        };

        // Touched set: CoW pointer walk when the snapshots share lineage
        // (O(divergence)), full semantic scan otherwise (e.g. the fork was
        // itself wire-ingested). The fast path is conservative, so every
        // candidate is re-checked by value below — false positives cost a
        // comparison, never bytes.
        let candidates: std::collections::BTreeSet<u32> =
            match crate::world::WorldSnapshot::touched_entities(&base, &fork) {
                Some(t) => t,
                None => {
                    let mut t = std::collections::BTreeSet::new();
                    t.extend(wb.all_entity_ids());
                    t.extend(wf.all_entity_ids());
                    t
                }
            };

        // An entity the base already holds travels as a surgical patch:
        // only the changed fields of the changed components (plus removed
        // component names). Full upsert rows remain for spawns, renames,
        // newly attached components, and layout drift.
        struct EntPatch {
            eid: u32,
            comps: Vec<(crate::value::ComponentData, Vec<usize>)>,
            removed: Vec<String>,
        }
        let mut despawns: Vec<u32> = Vec::new();
        let mut upserts: Vec<u32> = Vec::new();
        let mut ent_patches: Vec<EntPatch> = Vec::new();
        for &eid in &candidates {
            match (wb.contains_entity(eid), wf.contains_entity(eid)) {
                (true, false) => despawns.push(eid),
                (false, true) => upserts.push(eid),
                (true, true) => {
                    if wb.entity_name(eid) != wf.entity_name(eid) {
                        upserts.push(eid);
                        continue;
                    }
                    let bcomps = sorted_comps(&wb, eid);
                    let fcomps = sorted_comps(&wf, eid);
                    if bcomps == fcomps {
                        continue;
                    }
                    let mut patchable = true;
                    let mut comps: Vec<(crate::value::ComponentData, Vec<usize>)> = Vec::new();
                    for fc in &fcomps {
                        match bcomps.iter().find(|bc| bc.type_name == fc.type_name) {
                            // a component the base lacks: whole row needed
                            None => {
                                patchable = false;
                                break;
                            }
                            Some(bc) => {
                                if bc.layout != fc.layout || bc.values.len() != fc.values.len() {
                                    patchable = false;
                                    break;
                                }
                                let idxs: Vec<usize> = (0..fc.values.len())
                                    .filter(|&i| bc.values[i] != fc.values[i])
                                    .collect();
                                if !idxs.is_empty() {
                                    comps.push((fc.clone(), idxs));
                                }
                            }
                        }
                    }
                    if patchable {
                        let removed: Vec<String> = bcomps
                            .iter()
                            .filter(|bc| !fcomps.iter().any(|fc| fc.type_name == bc.type_name))
                            .map(|c| c.type_name.clone())
                            .collect();
                        ent_patches.push(EntPatch {
                            eid,
                            comps,
                            removed,
                        });
                    } else {
                        upserts.push(eid);
                    }
                }
                (false, false) => {}
            }
        }

        let mut changed_res: Vec<String> = Vec::new();
        {
            let mut rnames: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            rnames.extend(wb.resource_names());
            rnames.extend(wf.resource_names());
            for rname in rnames {
                match (wb.get_resource(&rname), wf.get_resource(&rname)) {
                    (Some(a), Some(b)) if a == b => {}
                    (None, None) => {}
                    (_, Some(_)) => changed_res.push(rname),
                    (_, None) => {} // resources are never removed
                }
            }
        }

        let mut schema: std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>> =
            std::collections::BTreeMap::new();
        let mut body = String::with_capacity(8 * 1024);

        // The base fingerprint: positional counters (cheap) PLUS a full
        // content digest (`bdig`). The counters alone let a resource-only
        // divergence slip through — the JS test harness caught exactly
        // that on its first run — so the digest is what actually refuses
        // out-of-order deltas. `sdig` scopes the digest to a schema
        // vintage: across a rolling migration the receiver's migrated
        // base legitimately hashes differently, so the content check
        // only binds same-schema peers.
        let bdig = Self::fork_digest(&base, &self.transient_resources)?;
        let sdig = self.schema_digest_value();
        let _ = write!(
            body,
            "{{\"check\":[{},{},{}],\"bdig\":\"{}\",\"sdig\":\"{}\",\"despawns\":[",
            base.next_id,
            base.entity_archetype.len(),
            base.events.len(),
            bdig,
            sdig
        );
        for (i, eid) in despawns.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "{}", eid);
        }

        body.push_str("],\"upserts\":[");
        for (i, &eid) in upserts.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "[{},", eid);
            match wf.entity_name(eid) {
                Some(name) => crate::wire::escape_json_into(&mut body, &name),
                None => body.push_str("null"),
            }
            body.push_str(",[");
            for (j, data) in sorted_comps(&wf, eid).iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                crate::wire::write_row_into(&mut schema, data, &mut body)
                    .map_err(|e| format!("fork_delta: {}", e))?;
                body.push(']');
            }
            body.push_str("]]");
        }

        // Surgical entity patches: [eid, [[comp, [[field, value]…]]…], [removed…]]
        // Patched components register in the schema table so the receiver can
        // detect shape drift and re-run its `migrate` block on patched rows.
        body.push_str("],\"ent_patch\":[");
        for (i, p) in ent_patches.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "[{},[", p.eid);
            for (j, (data, idxs)) in p.comps.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                schema.insert(data.type_name.clone(), data.layout.clone());
                body.push('[');
                crate::wire::escape_json_into(&mut body, &data.type_name);
                body.push_str(",[");
                for (k, &fi) in idxs.iter().enumerate() {
                    if k > 0 {
                        body.push(',');
                    }
                    body.push('[');
                    crate::wire::escape_json_into(&mut body, &data.layout[fi]);
                    body.push(',');
                    crate::wire::encode_value_into(&data.values[fi], &mut body)
                        .map_err(|e| format!("fork_delta: {}", e))?;
                    body.push(']');
                }
                body.push_str("]]");
            }
            body.push_str("],[");
            for (j, rname) in p.removed.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, rname);
            }
            body.push_str("]]");
        }

        // The in-flight queue ships whole (small, and append-only relative
        // to base); emit ids cross into the foreign namespace like the full
        // codec's.
        body.push_str("],\"events\":[");
        for (i, (name, payload, tid)) in fork.events.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, name);
            let _ = write!(
                body,
                ",{},{},",
                tid,
                crate::causality::foreign_emit_id(fork.emit_ids.get(i).copied().unwrap_or(0))
            );
            crate::wire::encode_value_into(payload, &mut body)
                .map_err(|e| format!("fork_delta: {}", e))?;
            body.push(']');
        }

        body.push_str("],\"delayed\":[");
        for (i, (left, name, payload, emit_id)) in fork.delayed.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            let _ = write!(body, "{},", left);
            crate::wire::escape_json_into(&mut body, name);
            let _ = write!(body, ",{},", crate::causality::foreign_emit_id(*emit_id));
            crate::wire::encode_value_into(payload, &mut body)
                .map_err(|e| format!("fork_delta: {}", e))?;
            body.push(']');
        }

        let mut free_ids = fork.free_ids.clone();
        free_ids.sort_unstable();
        body.push_str("],\"free_ids\":[");
        for (i, fid) in free_ids.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            let _ = write!(body, "{}", fid);
        }
        let _ = write!(body, "],\"next_id\":{},\"resources\":[", fork.next_id);

        // Changed resources travel as per-field patches when the base holds
        // the same layout: a 40-round battle journal must not re-ship its
        // whole log string because `round` ticked. Whole rows remain the
        // fallback for resources the base lacks (or whose layout differs).
        let mut patch_res: Vec<(&str, crate::value::ComponentData, Vec<usize>)> = Vec::new();
        let mut whole_res: Vec<&str> = Vec::new();
        for rname in &changed_res {
            match (wb.get_resource(rname), wf.get_resource(rname)) {
                (Some(a), Some(b)) if a.layout == b.layout && a.values.len() == b.values.len() => {
                    let idxs: Vec<usize> = (0..b.values.len())
                        .filter(|&i| a.values[i] != b.values[i])
                        .collect();
                    patch_res.push((rname.as_str(), b, idxs));
                }
                _ => whole_res.push(rname.as_str()),
            }
        }

        for (i, rname) in whole_res.iter().enumerate() {
            if let Some(data) = wf.get_resource(rname) {
                if i > 0 {
                    body.push(',');
                }
                body.push('[');
                crate::wire::write_row_into(&mut schema, &data, &mut body)
                    .map_err(|e| format!("fork_delta: {}", e))?;
                body.push(']');
            }
        }

        body.push_str("],\"res_patch\":[");
        for (i, (rname, data, idxs)) in patch_res.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, rname);
            body.push_str(",[");
            for (j, &fi) in idxs.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                crate::wire::escape_json_into(&mut body, &data.layout[fi]);
                body.push(',');
                crate::wire::encode_value_into(&data.values[fi], &mut body)
                    .map_err(|e| format!("fork_delta: {}", e))?;
                body.push(']');
            }
            body.push_str("]]");
        }

        body.push_str("],\"schema\":[");
        for (i, (tname, layout)) in schema.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, tname);
            body.push_str(",[");
            for (j, f) in layout.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, f);
            }
            body.push_str("]]");
        }

        // Provenance restricted to the divergence: the receiver already
        // holds the base's history (it ingested the base), so only records
        // for touched values need to travel.
        body.push_str("],\"prov\":");
        {
            let keep_ids: std::collections::HashSet<u32> = despawns
                .iter()
                .chain(upserts.iter())
                .chain(ent_patches.iter().map(|p| &p.eid))
                .copied()
                .collect();
            let keep_res: std::collections::HashSet<&str> =
                changed_res.iter().map(|s| s.as_str()).collect();
            let prov = match fork.provenance.as_deref() {
                Some(p) => {
                    // A wire-ingested fork carries its records; filter to
                    // the divergence, keep the emit chain whole (it is
                    // already a bounded closure).
                    let mut filtered = p.clone();
                    filtered.writes.retain(|w| match w.entity {
                        Some(e) => keep_ids.contains(&e),
                        None => keep_res.contains(w.component.as_str()),
                    });
                    filtered
                }
                None => self.ledger.provenance_closure(
                    |w| match w.entity {
                        Some(e) => keep_ids.contains(&e),
                        None => keep_res.contains(w.component.as_str()),
                    },
                    &fork
                        .emit_ids
                        .iter()
                        .copied()
                        .chain(fork.delayed.iter().map(|(_, _, _, id)| *id))
                        .collect::<Vec<_>>(),
                ),
            };
            crate::wire::encode_prov_into(&prov, &mut body);
        }
        body.push('}');

        let out = crate::radpack::seal("RADDELTA1", &body);
        Ok(Value::from_string(&mut self.gc, out))
    }

    /// `fork_apply(base, delta) -> Result<world_fork, str>` — delta sync,
    /// apply half. Rebuilds the sender's fork on top of the receiver's copy
    /// of the same base: CoW restore + only the shipped divergence, so the
    /// result **shares lineage with the local base** — the O(divergence)
    /// merge fast path works on wire-delivered forks. Shipped rows migrate
    /// on schema drift exactly like the full codec; corruption and
    /// wrong-base application are an `Err`, not a crash.
    pub(crate) fn bi_fork_apply(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "fork_apply() expects 2 arguments (base, delta), got {}",
                args.len()
            ));
        }
        let base = args[0]
            .as_world_fork()
            .cloned()
            .ok_or_else(|| "fork_apply() first argument must be a world_fork".to_string())?;
        let text = args[1]
            .as_str()
            .ok_or_else(|| format!("fork_apply() expects str, got {}", args[1].type_name()))?
            .to_string();

        match self.apply_fork_delta(&base, &text) {
            Ok(snap) => {
                let v = Value::world_fork(&mut self.gc, std::sync::Arc::new(snap));
                Ok(self.make_result(true, v))
            }
            Err(msg) => {
                let e = Value::from_string(&mut self.gc, msg);
                Ok(self.make_result(false, e))
            }
        }
    }

    fn apply_fork_delta(
        &mut self,
        base: &std::sync::Arc<crate::world::WorldSnapshot>,
        text: &str,
    ) -> Result<crate::world::WorldSnapshot, String> {
        let text = crate::radpack::open(text).map_err(|e| format!("fork_apply: {}", e))?;
        let text: &str = &text;
        let rest = text
            .strip_prefix("RADDELTA1 ")
            .ok_or("fork_apply: not a rad-delta payload (expected RADDELTA1 header)")?;
        let (claimed, body_text) = rest.split_once(' ').ok_or("fork_apply: malformed header")?;
        let actual = blake3::hash(body_text.as_bytes()).to_hex();
        if claimed != actual.as_str() {
            return Err(format!(
                "fork_apply: integrity digest mismatch (claimed {}…, computed {}…) — \
                 payload corrupted or tampered",
                crate::radpack::preview(claimed, 12),
                &actual.as_str()[..12]
            ));
        }
        let body: serde_json::Value = serde_json::from_str(body_text)
            .map_err(|e| format!("fork_apply: invalid JSON: {}", e))?;

        // Base fingerprint: a delta describes a divergence *from somewhere*;
        // applying it elsewhere would silently fabricate a world.
        let check = body["check"]
            .as_array()
            .filter(|a| a.len() == 3)
            .ok_or("fork_apply: malformed check section")?;
        let (cn, ce, cv) = (
            check[0].as_u64().unwrap_or(u64::MAX),
            check[1].as_u64().unwrap_or(u64::MAX),
            check[2].as_u64().unwrap_or(u64::MAX),
        );
        if cn != base.next_id as u64
            || ce != base.entity_archetype.len() as u64
            || cv != base.events.len() as u64
        {
            return Err(format!(
                "fork_apply: delta was made against a different base \
                 (expected allocator {} / {} entities / {} pending events, \
                  local base has {} / {} / {})",
                cn,
                ce,
                cv,
                base.next_id,
                base.entity_archetype.len(),
                base.events.len()
            ));
        }
        // Content digest: catches divergences the counters can't see
        // (resource changes, in-place component writes). Only binding
        // between same-schema peers — across a rolling migration the
        // receiver's migrated base hashes differently BY DESIGN, and the
        // migrate-on-ingest path re-shapes rows anyway. Absent in deltas
        // from older builds — those get counter-only checking.
        let same_schema = body["sdig"].as_str() == Some(self.schema_digest_value().as_str());
        if let (true, Some(claimed_bdig)) = (same_schema, body["bdig"].as_str()) {
            let local_bdig = Self::fork_digest(base, &self.transient_resources)?;
            if claimed_bdig != local_bdig {
                return Err(format!(
                    "fork_apply: delta was made against a different base \
                     (base digest {}… != local {}…) — apply deltas in order",
                    crate::radpack::preview(claimed_bdig, 12),
                    &local_bdig[..12]
                ));
            }
        }

        // Schema of shipped types only.
        let mut schema: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for entry in body["schema"].as_array().into_iter().flatten() {
            let pair = entry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_apply: malformed schema entry")?;
            let tname = pair[0]
                .as_str()
                .ok_or("fork_apply: malformed schema entry")?;
            let fields: Vec<String> = pair[1]
                .as_array()
                .ok_or("fork_apply: malformed schema entry")?
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            schema.insert(tname.to_string(), fields);
        }

        enum Plan {
            Direct {
                declared: std::sync::Arc<Vec<String>>,
                map: Vec<usize>,
            },
            Migrate {
                stored: Vec<String>,
                declared: std::sync::Arc<Vec<String>>,
            },
        }
        let make_plan = |stored: &[String], declared: std::sync::Arc<Vec<String>>| -> Plan {
            if stored.len() == declared.len() {
                let mut map = Vec::with_capacity(declared.len());
                for f in declared.iter() {
                    match stored.iter().position(|s| s == f) {
                        Some(i) => map.push(i),
                        None => {
                            return Plan::Migrate {
                                stored: stored.to_vec(),
                                declared,
                            }
                        }
                    }
                }
                Plan::Direct { declared, map }
            } else {
                Plan::Migrate {
                    stored: stored.to_vec(),
                    declared,
                }
            }
        };
        let mut plans: std::collections::HashMap<String, Plan> = std::collections::HashMap::new();

        // Allocator first, mutations second: each upsert can issue at most
        // one fresh id past the (trusted, local) base allocator, so the
        // delta's next_id is bounded by base + upsert count and every
        // shipped id must sit under it. Validating here — before any
        // insert — is what keeps a hostile id from flooding the free-list
        // gap-fill or overflowing the allocator (fuzzer finding).
        let upserts_len = body["upserts"].as_array().map_or(0, |a| a.len()) as u64;
        let next_id_u64 = body["next_id"]
            .as_u64()
            .ok_or("fork_apply: missing next_id")?;
        if next_id_u64 > base.next_id as u64 + upserts_len {
            return Err(format!(
                "fork_apply: delta allocator claims {} ids but base {} + {} \
                 upserts can issue at most {}",
                next_id_u64,
                base.next_id,
                upserts_len,
                base.next_id as u64 + upserts_len
            ));
        }
        let next_id = u32::try_from(next_id_u64)
            .map_err(|_| "fork_apply: next_id out of range".to_string())?;
        let mut free_ids: Vec<u32> = Vec::new();
        for v in body["free_ids"].as_array().into_iter().flatten() {
            let id = v
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("fork_apply: free id out of range")?;
            free_ids.push(id);
        }

        // CoW restore of the local base: untouched columns stay shared with
        // it, which is what keeps the later merge O(divergence).
        let mut w = crate::world::World::new();
        w.restore((**base).clone());

        for d in body["despawns"].as_array().into_iter().flatten() {
            // try_from, not `as`: a truncating cast would silently despawn
            // whatever entity the low 32 bits happen to name.
            let eid = d
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("fork_apply: malformed despawn id")?;
            if !w.contains_entity(eid) {
                return Err(format!(
                    "fork_apply: delta despawns entity {} which the base does not have",
                    eid
                ));
            }
            w.destroy_entity(eid);
        }

        for ent in body["upserts"].as_array().into_iter().flatten() {
            let parts = ent
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("fork_apply: malformed upsert entry")?;
            let eid_u64 = parts[0].as_u64().ok_or("fork_apply: upsert without id")?;
            let eid = u32::try_from(eid_u64)
                .ok()
                .filter(|&id| id < next_id)
                .ok_or_else(|| {
                    format!(
                        "fork_apply: upsert id {} is outside the allocator \
                         range (next_id {})",
                        eid_u64, next_id
                    )
                })?;
            let name = parts[1].as_str();
            let comps_json = parts[2]
                .as_array()
                .ok_or("fork_apply: malformed upsert components")?;

            let mut comps: Vec<crate::value::ComponentData> = Vec::with_capacity(comps_json.len());
            for centry in comps_json {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_apply: malformed component entry")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("fork_apply: malformed component entry")?;
                let vals = cpair[1]
                    .as_array()
                    .ok_or("fork_apply: malformed component values")?;

                if !plans.contains_key(cname) {
                    let stored = schema
                        .get(cname)
                        .ok_or_else(|| format!("fork_apply: no schema entry for '{}'", cname))?;
                    let declared = self.component_layouts.get(cname).cloned().ok_or_else(|| {
                        format!(
                            "fork_apply: delta contains component '{}' which is not \
                                 declared in this program",
                            cname
                        )
                    })?;
                    plans.insert(cname.to_string(), make_plan(stored, declared));
                }
                let data = match plans.get(cname).unwrap() {
                    Plan::Direct { declared, map } => {
                        if vals.len() != map.len() {
                            return Err(format!(
                                "fork_apply: '{}' row has {} values, schema says {}",
                                cname,
                                vals.len(),
                                map.len()
                            ));
                        }
                        let mut values = Vec::with_capacity(map.len());
                        for &si in map {
                            values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                        }
                        let data = crate::value::ComponentData {
                            type_name: cname.to_string(),
                            layout: declared.clone(),
                            values,
                        };
                        self.validate_loaded_row("fork_apply", &data)?;
                        data
                    }
                    Plan::Migrate { stored, declared } => {
                        let (stored, declared) = (stored.clone(), declared.clone());
                        self.migrate_wire_row(cname, &stored, declared, vals, 0)?
                    }
                };
                comps.push(data);
            }

            if w.contains_entity(eid) {
                // Surgical update against the base row: only differing
                // components move, so untouched columns stay CoW-shared.
                if w.entity_name(eid).as_deref() != name {
                    w.set_entity_name(eid, name);
                }
                let existing = w.components_on_entity(eid);
                let new_names: std::collections::HashSet<&str> =
                    comps.iter().map(|c| c.type_name.as_str()).collect();
                for old in &existing {
                    if !new_names.contains(old.type_name.as_str()) {
                        w.remove_component(eid, &old.type_name);
                    }
                }
                for data in comps {
                    let unchanged = existing
                        .iter()
                        .find(|c| c.type_name == data.type_name)
                        .is_some_and(|c| *c == data);
                    if !unchanged {
                        w.add_component(eid, data);
                    }
                }
            } else if !w.insert_entity_with_components(eid, name, comps) {
                return Err(format!("fork_apply: duplicate entity id {}", eid));
            }
        }

        // Surgical entity patches: change only the named fields of the named
        // components on rows the base already holds. Addressed by field name,
        // so only the receiver's declared layout matters.
        for pentry in body["ent_patch"].as_array().into_iter().flatten() {
            let parts = pentry
                .as_array()
                .filter(|a| a.len() == 3)
                .ok_or("fork_apply: malformed entity patch entry")?;
            // try_from, not `as`: a truncating cast would silently patch
            // whatever entity the low 32 bits happen to name.
            let eid = parts[0]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or("fork_apply: entity patch without id")?;
            if !w.contains_entity(eid) {
                return Err(format!(
                    "fork_apply: delta patches entity {} which the base does not have",
                    eid
                ));
            }
            let comps = parts[1]
                .as_array()
                .ok_or("fork_apply: malformed entity patch components")?;
            let removed = parts[2]
                .as_array()
                .ok_or("fork_apply: malformed entity patch removals")?;

            let existing = w.components_on_entity(eid);
            for centry in comps {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_apply: malformed entity patch component")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("fork_apply: malformed entity patch component")?;
                let fields = cpair[1]
                    .as_array()
                    .ok_or("fork_apply: malformed entity patch fields")?;
                let mut row = existing
                    .iter()
                    .find(|c| c.type_name == cname)
                    .cloned()
                    .ok_or_else(|| {
                        format!(
                            "fork_apply: delta patches component '{}' on entity {} \
                             which the base row does not carry",
                            cname, eid
                        )
                    })?;
                for f in fields {
                    let fp = f
                        .as_array()
                        .filter(|a| a.len() == 2)
                        .ok_or("fork_apply: malformed entity patch field")?;
                    let fname = fp[0]
                        .as_str()
                        .ok_or("fork_apply: malformed entity patch field")?;
                    let pos = row.layout.iter().position(|l| l == fname).ok_or_else(|| {
                        format!(
                            "fork_apply: component '{}' has no field '{}' on this \
                             machine — a delta can only patch fields that survive \
                             the receiver's migration",
                            cname, fname
                        )
                    })?;
                    row.values[pos] = crate::wire::decode_value(&mut self.gc, &fp[1])?;
                }
                // Shape drift: the sender wrote this row under a different
                // field set than ours. The patched row re-enters through the
                // declared `migrate` block, exactly like a shipped whole row
                // would — derived fields (e.g. shield = hp/2) stay coherent.
                let drifted = schema.get(cname).is_some_and(|sender| {
                    let a: std::collections::HashSet<&str> =
                        sender.iter().map(|s| s.as_str()).collect();
                    let b: std::collections::HashSet<&str> =
                        row.layout.iter().map(|s| s.as_str()).collect();
                    a != b
                });
                if drifted {
                    let declared = row.layout.clone();
                    let pairs: Vec<(String, Value)> = declared
                        .iter()
                        .cloned()
                        .zip(row.values.iter().cloned())
                        .collect();
                    // Patch payloads carry no schema versions: 0.
                    row = self.migrate_loaded(cname, pairs, &declared, 0)?;
                }
                self.validate_loaded_row("fork_apply", &row)?;
                w.add_component(eid, row);
            }
            for r in removed {
                let rname = r
                    .as_str()
                    .ok_or("fork_apply: malformed entity patch removal")?;
                w.remove_component(eid, rname);
            }
        }

        for rentry in body["resources"].as_array().into_iter().flatten() {
            let rpair = rentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_apply: malformed resource entry")?;
            let rname = rpair[0]
                .as_str()
                .ok_or("fork_apply: malformed resource entry")?;
            let vals = rpair[1]
                .as_array()
                .ok_or("fork_apply: malformed resource values")?;
            let stored = schema
                .get(rname)
                .ok_or_else(|| format!("fork_apply: no schema entry for '{}'", rname))?;
            let declared = self
                .world
                .get_resource(rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "fork_apply: delta contains resource '{}' which is not declared \
                         in this program",
                        rname
                    )
                })?;
            let data = match make_plan(stored, declared) {
                Plan::Direct { declared, map } => {
                    if vals.len() != map.len() {
                        return Err(format!(
                            "fork_apply: resource '{}' has {} values, schema says {}",
                            rname,
                            vals.len(),
                            map.len()
                        ));
                    }
                    let mut values = Vec::with_capacity(map.len());
                    for si in map {
                        values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                    }
                    let data = crate::value::ComponentData {
                        type_name: rname.to_string(),
                        layout: declared,
                        values,
                    };
                    self.validate_loaded_row("fork_apply", &data)?;
                    data
                }
                Plan::Migrate { stored, declared } => {
                    self.migrate_wire_row(rname, &stored, declared, vals, 0)?
                }
            };
            w.set_resource(rname, data);
        }

        // Per-field resource patches: surgical edits against the base row,
        // addressed by field name so the receiver's declared layout is the
        // only one that matters.
        for pentry in body["res_patch"].as_array().into_iter().flatten() {
            let pair = pentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("fork_apply: malformed resource patch entry")?;
            let rname = pair[0]
                .as_str()
                .ok_or("fork_apply: malformed resource patch entry")?;
            let fields = pair[1]
                .as_array()
                .ok_or("fork_apply: malformed resource patch fields")?;
            let mut row = w.get_resource(rname).ok_or_else(|| {
                format!(
                    "fork_apply: delta patches resource '{}' which the base does not have",
                    rname
                )
            })?;
            for f in fields {
                let fp = f
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("fork_apply: malformed resource patch field")?;
                let fname = fp[0]
                    .as_str()
                    .ok_or("fork_apply: malformed resource patch field")?;
                let pos = row.layout.iter().position(|l| l == fname).ok_or_else(|| {
                    format!(
                        "fork_apply: resource '{}' has no field '{}' \
                         (schema drift across a delta session is not supported)",
                        rname, fname
                    )
                })?;
                row.values[pos] = crate::wire::decode_value(&mut self.gc, &fp[1])?;
            }
            self.validate_loaded_row("fork_apply", &row)?;
            w.set_resource(rname, row);
        }

        w.set_id_allocator(next_id, free_ids)
            .map_err(|e| format!("fork_apply: {}", e))?;

        let mut events: Vec<(String, Value, u64)> = Vec::new();
        let mut emit_ids: Vec<u64> = Vec::new();
        for ev in body["events"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 4)
                .ok_or("fork_apply: malformed event entry")?;
            let name = parts[0]
                .as_str()
                .ok_or("fork_apply: event without name")?
                .to_string();
            let tid = parts[1].as_u64().unwrap_or(0);
            let emit_id = parts[2].as_u64().unwrap_or(0);
            let payload = crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[3])?;
            events.push((name, payload, tid));
            emit_ids.push(emit_id);
        }

        let mut delayed: Vec<(i64, String, Value, u64)> = Vec::new();
        for ev in body["delayed"].as_array().into_iter().flatten() {
            let parts = ev
                .as_array()
                .filter(|a| a.len() == 3 || a.len() == 4)
                .ok_or("fork_apply: malformed delayed entry")?;
            let left = parts[0]
                .as_i64()
                .ok_or("fork_apply: delayed entry without tick count")?;
            let name = parts[1]
                .as_str()
                .ok_or("fork_apply: delayed entry without name")?
                .to_string();
            let (emit_id, payload_idx) = if parts.len() == 4 {
                (parts[2].as_u64().unwrap_or(0), 3)
            } else {
                (0, 2)
            };
            let payload =
                crate::wire::decode_value(&mut crate::value::PersistentStore, &parts[payload_idx])?;
            delayed.push((left, name, payload, emit_id));
        }

        let mut snap = w.snapshot();
        snap.events = std::sync::Arc::new(events);
        snap.emit_ids = std::sync::Arc::new(emit_ids);
        snap.delayed = std::sync::Arc::new(delayed);
        if let Some(pj) = body.get("prov") {
            let mut prov = crate::wire::decode_prov(pj)?;
            prov.origin = format!("wire {}", crate::radpack::preview(claimed, 8));
            snap.provenance = Some(std::sync::Arc::new(prov));
        }
        Ok(snap)
    }

    /// `save_world() -> str` â€” schema migration (#5), the save half.
    /// Serializes entities (names + components) and resources to JSON with
    /// the **schema embedded** (per-type field layout), using the tagged
    /// value codec for full fidelity. Pure given the world: persistence
    /// composes with io (`write_file(path, save_world())`), so record &
    /// replay needs no new machinery.
    pub(crate) fn bi_save_world(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("save_world() takes no arguments".into());
        }
        // A full world dump — the strongest bulk read there is.
        self.sandbox_check_bulk_read("save_world()")?;
        let body = self.save_world_body()?;
        // RADWORLD3 carries a blake3 integrity digest (`RADWORLD3 <digest>
        // <body>`, or the RADPACK1 envelope for large saves), closing the
        // gap where a small `.radw` save had none and silently accepted
        // corruption (dogfood feature seq 69). `seal` puts the digest in the
        // legacy form for every tag except RADWORLD2, so switching the tag is
        // all it takes. `load_world` verifies RADWORLD3 and still reads the
        // digest-less RADWORLD2 and the v1 tagged-tree forever.
        let out = crate::radpack::seal("RADWORLD3", &body);
        Ok(Value::from_string(&mut self.gc, out))
    }

    /// The canonical state-only serialization (the `RADWORLD2` body).
    /// Shared by `save_world` (which envelopes it) and `world_digest`
    /// (which hashes it — keeping the digest independent of transport
    /// encoding decisions).
    fn save_world_body(&mut self) -> Result<String, String> {
        let skip = Arc::clone(&self.transient_resources);
        let versions = Arc::clone(&self.component_versions);
        Self::world_body_of(&self.world, &skip, &versions)
    }

    /// Canonical state-only body of ANY world — the live one for
    /// `save_world()` / `world_digest()`, or a fork's reconstruction for
    /// `world_digest(fork)` (the cross-version convergence certificate:
    /// a server digests the migrated view of a peer's world without
    /// committing it).
    /// `versions`: declared schema versions to embed per type in the schema
    /// section (`["T",["f"],2]`, dogfood seq 69). save_world passes the
    /// program's declarations; DIGEST callers pass an empty map — a version
    /// tag is load metadata, not state, so re-tagging a component must not
    /// change `world_digest()` (state identity survives a rolling upgrade).
    fn world_body_of(
        world: &crate::world::World,
        skip_resources: &std::collections::HashSet<String>,
        versions: &std::collections::HashMap<String, u32>,
    ) -> Result<String, String> {
        // v2: `RADWORLD2 {body}` â€” direct string writer with the compact
        // wire value codec (same machinery as `fork_to_bytes`); the v1
        // tagged-tree format is still accepted by `load_world`.
        let mut schema: std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>> =
            std::collections::BTreeMap::new();
        fn write_data(
            schema: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>>,
            data: &crate::value::ComponentData,
            out: &mut String,
        ) -> Result<(), String> {
            let wire_layout = schema
                .entry(data.type_name.clone())
                .or_insert_with(|| data.layout.clone())
                .clone();
            crate::wire::escape_json_into(out, &data.type_name);
            out.push_str(",[");
            let aligned =
                std::sync::Arc::ptr_eq(&wire_layout, &data.layout) || *wire_layout == *data.layout;
            for i in 0..wire_layout.len() {
                if i > 0 {
                    out.push(',');
                }
                let v = if aligned {
                    &data.values[i]
                } else {
                    let f = &wire_layout[i];
                    let pos = data.layout.iter().position(|n| n == f).ok_or_else(|| {
                        format!(
                            "save_world: instances of '{}' disagree on field '{}'",
                            data.type_name, f
                        )
                    })?;
                    &data.values[pos]
                };
                crate::wire::encode_value_into(v, out)?;
            }
            out.push(']');
            Ok(())
        }

        let mut body = String::with_capacity(64 * 1024);
        body.push_str("{\"entities\":[");
        for (i, eid) in world.all_entity_ids().into_iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            match world.entity_name(eid) {
                Some(name) => crate::wire::escape_json_into(&mut body, &name),
                None => body.push_str("null"),
            }
            body.push_str(",[");
            let mut comps = world.components_on_entity(eid);
            comps.sort_by(|a, b| a.type_name.cmp(&b.type_name));
            for (j, data) in comps.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                body.push('[');
                write_data(&mut schema, data, &mut body)?;
                body.push(']');
            }
            body.push_str("]]");
        }

        body.push_str("],\"resources\":[");
        let mut rnames = world.resource_names();
        rnames.sort();
        // transient resources are not part of the world's identity
        rnames.retain(|n| !skip_resources.contains(n));
        let mut first = true;
        for rname in rnames.iter() {
            if let Some(data) = world.get_resource(rname) {
                if !first {
                    body.push(',');
                }
                first = false;
                body.push('[');
                write_data(&mut schema, &data, &mut body)?;
                body.push(']');
            }
        }

        body.push_str("],\"schema\":[");
        for (i, (tname, layout)) in schema.iter().enumerate() {
            if i > 0 {
                body.push(',');
            }
            body.push('[');
            crate::wire::escape_json_into(&mut body, tname);
            body.push_str(",[");
            for (j, f) in layout.iter().enumerate() {
                if j > 0 {
                    body.push(',');
                }
                crate::wire::escape_json_into(&mut body, f);
            }
            body.push(']');
            // Optional third element: the declared schema version (only
            // nonzero versions are recorded, so undeclared programs emit
            // byte-identical saves).
            if let Some(v) = versions.get(tname.as_str()) {
                body.push(',');
                body.push_str(&v.to_string());
            }
            body.push(']');
        }
        body.push_str("]}");
        Ok(body)
    }

    /// blake3 of the canonical state-only serialization (`save_world` body).
    /// Excludes events, provenance, frame counters, and id free-lists — the
    /// convergence receipt for distributed sync: machines that merged to the
    /// same world print the same digest even though their fork bytes differ.
    /// Content digest of a frozen fork — same recipe as `world_digest`,
    /// usable wherever a snapshot needs a convergence fingerprint.
    pub(crate) fn fork_digest(
        snap: &std::sync::Arc<crate::world::WorldSnapshot>,
        skip_resources: &std::collections::HashSet<String>,
    ) -> Result<String, String> {
        let mut scratch = crate::world::World::new();
        scratch.restore((**snap).clone());
        let no_versions = std::collections::HashMap::new();
        let body = Self::world_body_of(&scratch, skip_resources, &no_versions)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"RADWORLD2 ");
        hasher.update(body.as_bytes());
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub(crate) fn bi_world_digest(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() > 1 {
            return Err(format!(
                "world_digest() takes 0 arguments (live world) or 1 (a world_fork), got {}",
                args.len()
            ));
        }
        let body = match args.first() {
            None => {
                // No-arg form hashes the whole live world; the fork-arg form
                // hashes a fork the guest already holds, so only the former
                // is a world read. Version-free body (NOT save_world_body):
                // a version tag is load metadata, so world_digest() must
                // stay equal across peers that differ only in declared
                // versions — and equal to world_digest(fork) of the same
                // state.
                self.sandbox_check_bulk_read("world_digest()")?;
                let skip = Arc::clone(&self.transient_resources);
                let no_versions = std::collections::HashMap::new();
                Self::world_body_of(&self.world, &skip, &no_versions)?
            }
            Some(v) => {
                // `world_digest(fork)`: digest a fork's state without
                // committing it. The rolling-migration receipt: a v2 server
                // decodes a v1 peer's bytes (migrate-on-ingest shapes them
                // to v2), then digests THAT view — comparable against its
                // own digest, because both sides of the comparison now
                // carry the same schema.
                let snap = v.as_world_fork().ok_or_else(|| {
                    format!(
                        "world_digest() argument must be a world_fork, got {}",
                        v.type_name()
                    )
                })?;
                let mut scratch = crate::world::World::new();
                scratch.restore((**snap).clone());
                let skip = Arc::clone(&self.transient_resources);
                let no_versions = std::collections::HashMap::new();
                Self::world_body_of(&scratch, &skip, &no_versions)?
            }
        };
        // Hash the legacy `RADWORLD2 <body>` form — the exact bytes this
        // builtin hashed before RADPACK existed. The prefix is constant, so
        // the digest is still independent of envelope decisions, and tapes
        // recorded by older builds keep replaying byte-for-byte.
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"RADWORLD2 ");
        hasher.update(body.as_bytes());
        let digest = hasher.finalize().to_hex().to_string();
        Ok(Value::from_string(&mut self.gc, digest))
    }

    /// `schema_digest() -> str` — the PROGRAM's schema fingerprint: blake3
    /// of the declared component/resource/event layouts, sorted. Two peers
    /// with equal `schema_digest` may compare `world_digest` directly; when
    /// the fingerprints differ (a rolling migration), a raw digest mismatch
    /// means "different schema vintage", not "diverged" — certify through
    /// `world_digest(fork)` on the migrated view instead.
    pub(crate) fn bi_schema_digest(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("schema_digest() takes no arguments".into());
        }
        let digest = self.schema_digest_value();
        Ok(Value::from_string(&mut self.gc, digest))
    }

    pub(crate) fn schema_digest_value(&self) -> String {
        let mut names: Vec<&String> = self.component_layouts.keys().collect();
        names.sort();
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"RADSCHEMA1");
        for name in names {
            hasher.update(b"\x1f");
            hasher.update(name.as_bytes());
            for field in self.component_layouts[name].iter() {
                hasher.update(b"\x1e");
                hasher.update(field.as_bytes());
            }
        }
        hasher.finalize().to_hex().to_string()
    }

    /// `load_world(json) -> int` â€” the load half of schema migration (#5).
    /// For each persisted component/resource: identical shape loads as-is
    /// (field order normalized); shape drift runs the declared
    /// `migrate X(old)` block (old fields as `map<str, any>`); drift with
    /// no migration is a loud error naming the added/removed fields.
    /// Returns the number of entities loaded.
    pub(crate) fn bi_load_world(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("load_world() requires 1 argument (the save_world() JSON)".into());
        }
        let text = args[0]
            .as_str()
            .ok_or_else(|| format!("load_world() expects str, got {}", args[0].type_name()))?
            .to_string();
        let text = crate::radpack::open(&text)
            .map_err(|e| format!("load_world(): {}", e))?
            .into_owned();
        if let Some(rest) = text.strip_prefix("RADWORLD3 ") {
            // `RADWORLD3 <blake3-of-body> <body>`: verify the integrity
            // envelope before trusting a byte of the payload. (For a packed
            // save, radpack::open already checked and unwrapped it to this
            // legacy form; re-verifying the same digest is cheap and keeps
            // the one code path.)
            let (claimed, body) = rest.split_once(' ').ok_or_else(|| {
                "load_world(): malformed RADWORLD3 envelope (missing digest separator)".to_string()
            })?;
            let actual = blake3::hash(body.as_bytes()).to_hex();
            if claimed != actual.as_str() {
                return Err(format!(
                    "load_world(): integrity digest mismatch (claimed {}…, computed {}…) — \
                     save corrupted or tampered",
                    crate::radpack::preview(claimed, 12),
                    &actual.as_str()[..12]
                ));
            }
            return self.load_world_v2(body);
        }
        if let Some(body) = text.strip_prefix("RADWORLD2 ") {
            return self.load_world_v2(body);
        }
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("load_world(): invalid JSON: {}", e))?;
        let version = doc["version"].as_u64().unwrap_or(0);
        if version != 1 {
            return Err(format!(
                "load_world(): unsupported save version {} (expected 1)",
                version
            ));
        }
        let schema = doc["schema"]
            .as_object()
            .ok_or("load_world(): save has no schema")?
            .clone();
        let stored_layout = |tname: &str| -> Result<Vec<String>, String> {
            schema
                .get(tname)
                .and_then(|l| l.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .ok_or_else(|| format!("load_world(): no schema entry for '{}'", tname))
        };

        let mut target = self.load_world_replacement_target();
        let mut writes: Vec<(Option<u32>, String, crate::causality::WriteKind, String)> =
            Vec::new();
        let mut loaded = 0i64;
        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ent in doc["entities"].as_array().cloned().unwrap_or_default() {
            let name = ent.get("name").and_then(|n| n.as_str()).map(String::from);
            Self::validate_loaded_entity_name("load_world()", name.as_deref(), &mut seen_names)?;
            let eid = target.spawn_entity(name.as_deref());
            loaded += 1;
            let comps = ent["components"].as_object().cloned().unwrap_or_default();
            for (cname, fields) in comps {
                let stored = stored_layout(&cname)?;
                let declared = self.component_layouts.get(&cname).cloned().ok_or_else(|| {
                    format!(
                        "load_world(): save contains component '{}' which is not declared \
                             in this program",
                        cname
                    )
                })?;
                let data = self.realize_loaded(&cname, &stored, declared, &fields)?;
                let summary = Self::component_summary(&data);
                let _ = target.add_component(eid, data);
                writes.push((
                    Some(eid),
                    cname,
                    crate::causality::WriteKind::Spawn,
                    summary,
                ));
            }
        }

        for (rname, fields) in doc["resources"].as_object().cloned().unwrap_or_default() {
            let stored = stored_layout(&rname)?;
            // A resource's current shape comes from its live declaration-
            // initialized instance.
            let declared = self
                .world
                .get_resource(&rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "load_world(): save contains resource '{}' which is not declared \
                         in this program",
                        rname
                    )
                })?;
            let data = self.realize_loaded(&rname, &stored, declared, &fields)?;
            let summary = Self::component_summary(&data);
            target.set_resource(&rname, data);
            writes.push((None, rname, crate::causality::WriteKind::Resource, summary));
        }

        self.world = target;
        for (entity, component, kind, summary) in writes {
            self.record_causal_write(entity, &component, kind, summary);
        }
        Ok(Value::from_int(&mut self.gc, loaded))
    }

    /// `try_load_world(json) -> Result<int, str>` — the fallible sibling of
    /// `load_world()`. `load_world()` aborts on bad input, which made it the
    /// only deserialization entry point in the language that could not be
    /// handled in Rad — every other boundary (`fork_from_bytes`,
    /// `fork_apply`, `merge_forks`, `sandbox_run`, `json_parse`) returns a
    /// value (dogfood feature seq 69). This returns `Ok(entities_loaded)` or
    /// `Err(message)`, so an app can fall back to a prior backup when today's
    /// save is corrupt. `load_world` builds a replacement world and swaps it
    /// in only on success, so a failed load leaves the live world untouched —
    /// the property the fallback pattern depends on.
    fn bi_try_load_world(&mut self, args: Vec<Value>) -> Result<Value, String> {
        match self.bi_load_world(args) {
            Ok(v) => Ok(self.make_result(true, v)),
            Err(e) => {
                let msg = Value::from_string(&mut self.gc, e);
                Ok(self.make_result(false, msg))
            }
        }
    }

    /// Build the replacement world used by `load_world`.
    ///
    /// Saves carry entities and non-transient resources, but the program's
    /// schema-level runtime declarations live outside the file. Seed the fresh
    /// world with the current indexed-field declarations and declared resources
    /// so resource shape checks, transient resources, and omitted legacy
    /// resources remain available while loaded rows replace the entity set.
    fn load_world_replacement_target(&self) -> crate::world::World {
        let mut target = crate::world::World::new();
        target.set_indexed_fields_arc(Arc::clone(&self.indexed_decl));
        for rname in self.world.resource_names() {
            if let Some(data) = self.world.get_resource(&rname) {
                target.set_resource(&rname, data);
            }
        }
        target
    }

    /// Structural conformance of a deserialized value to a declared field
    /// type. Strict on scalars (a str is never an int, an int is never a
    /// bool, nil only satisfies nil-able types) with one deliberate
    /// exception: a float-declared field accepts an int — the checker
    /// allows that lossless direction at construction time, so well-formed
    /// worlds legitimately hold ints there. Types the checker cannot fully
    /// describe (Any, type variables, fn/task values that the wire codec
    /// refuses to encode anyway) validate permissively — the boundary must
    /// never wrongly reject a save the program legally produced.
    fn loaded_value_conforms(v: &Value, ty: &crate::types::Ty) -> bool {
        use crate::types::Ty;
        match ty {
            Ty::Int => v.as_int().is_some(),
            Ty::Float => v.as_float().is_some() || v.as_int().is_some(),
            Ty::Str => v.as_str().is_some(),
            Ty::Bool => v.as_bool().is_some(),
            Ty::Nil => v.is_nil(),
            Ty::EntityId => v.as_entity_id().is_some(),
            Ty::List(elem) => v
                .as_list()
                .is_some_and(|items| items.iter().all(|it| Self::loaded_value_conforms(it, elem))),
            Ty::Tuple(elems) => v.as_tuple().is_some_and(|items| {
                items.len() == elems.len()
                    && items
                        .iter()
                        .zip(elems)
                        .all(|(it, t)| Self::loaded_value_conforms(it, t))
            }),
            Ty::Map(kty, vty) => v.as_map().is_some_and(|m| {
                m.keys().all(|k| {
                    Self::loaded_map_key_conforms(k, kty) && Self::loaded_value_conforms(&m[k], vty)
                })
            }),
            Ty::Component(name) | Ty::Struct(name) => {
                v.as_component().is_some_and(|c| c.type_name == *name)
            }
            Ty::SumType(name) => v.as_sum_type().is_some_and(|st| st.type_name == *name),
            Ty::Union(alts) => alts.iter().any(|t| Self::loaded_value_conforms(v, t)),
            // Generic application: check the head name when the value
            // carries one; parameter checking would need instantiation.
            Ty::App(name, _) => {
                if let Some(st) = v.as_sum_type() {
                    st.type_name == *name
                } else if let Some(c) = v.as_component() {
                    c.type_name == *name
                } else {
                    true
                }
            }
            _ => true,
        }
    }

    fn loaded_map_key_conforms(k: &MapKey, ty: &crate::types::Ty) -> bool {
        use crate::types::Ty;
        match ty {
            Ty::Str => matches!(k, MapKey::Str(_)),
            Ty::Int => matches!(k, MapKey::Int(_)),
            Ty::Bool => matches!(k, MapKey::Bool(_)),
            Ty::EntityId => matches!(k, MapKey::Entity(_)),
            Ty::Tuple(elems) => match k {
                MapKey::Tuple(items) => {
                    items.len() == elems.len()
                        && items
                            .iter()
                            .zip(elems)
                            .all(|(i, t)| Self::loaded_map_key_conforms(i, t))
                }
                _ => false,
            },
            Ty::Union(alts) => alts.iter().any(|t| Self::loaded_map_key_conforms(k, t)),
            _ => true,
        }
    }

    /// Entity names are unique identity: `spawn` refuses to record an empty
    /// name and the name maps hold one id per name, so a well-formed save or
    /// fork payload can never carry `""` or a duplicate. Loading such a
    /// payload used to strip the name from one entity silently (the loser
    /// became unreachable via `get_entity`) — data loss with a success
    /// return. Refuse it, naming the collision.
    fn validate_loaded_entity_name(
        ctx: &str,
        name: Option<&str>,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        let Some(n) = name else { return Ok(()) };
        if n.is_empty() {
            return Err(format!(
                "{}: payload contains an entity named \"\" — a live world cannot hold an \
                 empty name (unnamed entities are stored as null); the payload is corrupt",
                ctx
            ));
        }
        if !seen.insert(n.to_string()) {
            return Err(format!(
                "{}: payload contains two entities named '{}' — entity names are unique \
                 identity, and loading both would silently strip the name from one of them",
                ctx, n
            ));
        }
        Ok(())
    }

    /// Enforce declared field types on a row crossing the deserialization
    /// boundary (`load_world`, fork bytes, a delta, or a `migrate` block's
    /// return value). Shape drift already fails loudly; this closes the
    /// remaining hole where the field SET matches but a value's TYPE does
    /// not — which would otherwise plant wrong-typed data in fields the
    /// static checker trusts. Rows of types the checker never described
    /// (checker-less compiles) validate permissively.
    /// Sandbox write-shape enforcement (capability model, list item #1).
    ///
    /// The component-write ACL (`sandbox_check_write`) only gates the type
    /// *name*. That let a guest declare its own version of a granted
    /// component and write it with the wrong field types (poisoning a
    /// statically-typed host field), a different field name (positional
    /// aliasing — guest fields land in host columns by position), or an
    /// extra/short field set (silently dropped). None of the documented
    /// host-side defenses (peek/diff/assert_only_changed) can see any of it.
    ///
    /// This binds a granted write to the HOST's declared schema: the guest
    /// (whose own `component_field_types` was overwritten with the host's in
    /// `run_sandbox_guest`) must write the exact declared field set, with
    /// each value conforming to the declared type. Host-unknown components
    /// (no declared schema) are left alone — the host chose to grant that
    /// name, and there is no schema to bind against. No-op outside a sandbox.
    pub(crate) fn sandbox_check_write_shape(
        &self,
        data: &crate::value::ComponentData,
    ) -> Result<(), String> {
        if self.sandbox_caps.is_none() {
            return Ok(());
        }
        let Some(decl) = self.component_field_types.get(&data.type_name) else {
            return Ok(());
        };
        let declared: std::collections::HashSet<&str> =
            decl.iter().map(|(n, _)| n.as_str()).collect();
        let written: std::collections::HashSet<&str> =
            data.layout.iter().map(|s| s.as_str()).collect();
        if declared != written {
            let mut d: Vec<&str> = declared.into_iter().collect();
            d.sort_unstable();
            let mut w: Vec<&str> = written.into_iter().collect();
            w.sort_unstable();
            return Err(format!(
                "sandbox: write to component '{}' uses fields {:?}, but the host declares {:?} — \
                 a granted component must be written with the host's exact schema \
                 (guest field aliasing or field drift is rejected at the boundary)",
                data.type_name, w, d
            ));
        }
        for (field, value) in data.layout.iter().zip(&data.values) {
            let Some((_, ty)) = decl.iter().find(|(n, _)| n == field) else {
                continue;
            };
            if !Self::loaded_value_conforms(value, ty) {
                let shown = format!("{}", value);
                return Err(format!(
                    "sandbox: write to '{}.{}' has value {} ({}), but the host declares {} — \
                     refusing to plant wrong-typed data in a statically-typed field through a \
                     capability grant",
                    data.type_name,
                    field,
                    crate::radpack::preview(&shown, 48),
                    value.type_name(),
                    ty,
                ));
            }
        }
        Ok(())
    }

    fn validate_loaded_row(
        &self,
        ctx: &str,
        data: &crate::value::ComponentData,
    ) -> Result<(), String> {
        let Some(decl) = self.component_field_types.get(&data.type_name) else {
            return Ok(());
        };
        for (field, value) in data.layout.iter().zip(&data.values) {
            let Some((_, ty)) = decl.iter().find(|(n, _)| n == field) else {
                continue;
            };
            if !Self::loaded_value_conforms(value, ty) {
                let shown = format!("{}", value);
                return Err(format!(
                    "{}: type drift in '{}.{}': declared {}, loaded value is {} ({}) — \
                     refusing to plant wrong-typed data in a statically-typed field",
                    ctx,
                    data.type_name,
                    field,
                    ty,
                    value.type_name(),
                    crate::radpack::preview(&shown, 48),
                ));
            }
        }
        Ok(())
    }

    /// Realize one persisted component/resource against the current schema:
    /// same field set â†’ load (order-normalized); drift â†’ run the `migrate`
    /// block or fail loudly.
    fn realize_loaded(
        &mut self,
        tname: &str,
        stored: &[String],
        declared: std::sync::Arc<Vec<String>>,
        fields: &serde_json::Value,
    ) -> Result<crate::value::ComponentData, String> {
        let fields = fields
            .as_object()
            .ok_or_else(|| format!("load_world(): fields of '{}' must be an object", tname))?;

        let mut stored_sorted: Vec<&String> = stored.iter().collect();
        stored_sorted.sort();
        let mut declared_sorted: Vec<&String> = declared.iter().collect();
        declared_sorted.sort();

        if stored_sorted == declared_sorted {
            // Same shape: decode straight into the declared field order.
            let mut values = Vec::with_capacity(declared.len());
            for f in declared.iter() {
                let j = fields
                    .get(f)
                    .ok_or_else(|| format!("load_world(): '{}' is missing field '{}'", tname, f))?;
                values.push(crate::replay::decode_value(&mut self.gc, j)?);
            }
            let data = crate::value::ComponentData {
                type_name: tname.to_string(),
                layout: declared,
                values,
            };
            self.validate_loaded_row("load_world()", &data)?;
            return Ok(data);
        }

        // Shape drift: decode the stored fields and hand them to the shared
        // migration path.
        let mut pairs = Vec::with_capacity(stored.len());
        for f in stored {
            let j = fields
                .get(f)
                .ok_or_else(|| format!("load_world(): '{}' is missing field '{}'", tname, f))?;
            pairs.push((f.clone(), crate::replay::decode_value(&mut self.gc, j)?));
        }
        // v1 tagged saves predate schema versions: 0 = "no declared version".
        self.migrate_loaded(tname, pairs, &declared, 0)
    }

    /// `load_world` fast path for v2 saves (`RADWORLD2 {body}`): one parse,
    /// one realize-plan per type, one archetype hop per entity. Migration
    /// semantics are identical to v1.
    fn load_world_v2(&mut self, body_text: &str) -> Result<Value, String> {
        let body: serde_json::Value = serde_json::from_str(body_text)
            .map_err(|e| format!("load_world(): invalid JSON: {}", e))?;

        let mut schema: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        // Optional third element per entry: the type's declared schema
        // version at save time (`component X v2`, dogfood seq 69) — handed
        // to `migrate X(old, from_version)`. Absent = 0.
        let mut schema_versions: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for entry in body["schema"].as_array().into_iter().flatten() {
            let pair = entry
                .as_array()
                .filter(|a| a.len() == 2 || a.len() == 3)
                .ok_or("load_world(): malformed schema entry")?;
            let tname = pair[0]
                .as_str()
                .ok_or("load_world(): malformed schema entry")?;
            let fields: Vec<String> = pair[1]
                .as_array()
                .ok_or("load_world(): malformed schema entry")?
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            if let Some(v) = pair.get(2).and_then(|v| v.as_i64()) {
                schema_versions.insert(tname.to_string(), v);
            }
            schema.insert(tname.to_string(), fields);
        }

        // Plan per type: aligned field sets decode positionally; drift runs
        // the `migrate` block per row.
        enum Plan {
            Direct {
                declared: std::sync::Arc<Vec<String>>,
                map: Vec<usize>,
            },
            Migrate {
                stored: Vec<String>,
                declared: std::sync::Arc<Vec<String>>,
            },
        }
        fn make_plan(stored: &[String], declared: std::sync::Arc<Vec<String>>) -> Plan {
            if stored.len() == declared.len() {
                let mut map = Vec::with_capacity(declared.len());
                for f in declared.iter() {
                    match stored.iter().position(|s| s == f) {
                        Some(i) => map.push(i),
                        None => {
                            return Plan::Migrate {
                                stored: stored.to_vec(),
                                declared,
                            }
                        }
                    }
                }
                Plan::Direct { declared, map }
            } else {
                Plan::Migrate {
                    stored: stored.to_vec(),
                    declared,
                }
            }
        }
        let mut plans: std::collections::HashMap<String, Plan> = std::collections::HashMap::new();

        let mut target = self.load_world_replacement_target();
        let mut writes: Vec<(Option<u32>, String, crate::causality::WriteKind, String)> =
            Vec::new();
        let mut loaded = 0i64;
        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ent in body["entities"].as_array().into_iter().flatten() {
            let parts = ent
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("load_world(): malformed entity entry")?;
            let name = parts[0].as_str();
            Self::validate_loaded_entity_name("load_world()", name, &mut seen_names)?;
            let comps_json = parts[1]
                .as_array()
                .ok_or("load_world(): malformed entity components")?;

            let mut comps: Vec<crate::value::ComponentData> = Vec::with_capacity(comps_json.len());
            for centry in comps_json {
                let cpair = centry
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or("load_world(): malformed component entry")?;
                let cname = cpair[0]
                    .as_str()
                    .ok_or("load_world(): malformed component entry")?;
                let vals = cpair[1]
                    .as_array()
                    .ok_or("load_world(): malformed component values")?;

                if !plans.contains_key(cname) {
                    let stored = schema
                        .get(cname)
                        .ok_or_else(|| format!("load_world(): no schema entry for '{}'", cname))?;
                    let declared = self.component_layouts.get(cname).cloned().ok_or_else(|| {
                        format!(
                            "load_world(): save contains component '{}' which is not \
                                 declared in this program",
                            cname
                        )
                    })?;
                    plans.insert(cname.to_string(), make_plan(stored, declared));
                }
                let data = match plans.get(cname).unwrap() {
                    Plan::Direct { declared, map } => {
                        if vals.len() != map.len() {
                            return Err(format!(
                                "load_world(): '{}' row has {} values, schema says {}",
                                cname,
                                vals.len(),
                                map.len()
                            ));
                        }
                        let mut values = Vec::with_capacity(map.len());
                        for &si in map {
                            values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                        }
                        let data = crate::value::ComponentData {
                            type_name: cname.to_string(),
                            layout: declared.clone(),
                            values,
                        };
                        self.validate_loaded_row("load_world()", &data)?;
                        data
                    }
                    Plan::Migrate { stored, declared } => {
                        let (stored, declared) = (stored.clone(), declared.clone());
                        let from_version = *schema_versions.get(cname).unwrap_or(&0);
                        self.migrate_wire_row(cname, &stored, declared, vals, from_version)?
                    }
                };
                comps.push(data);
            }

            let eid = target.spawn_entity(name);
            loaded += 1;
            for data in &comps {
                writes.push((
                    Some(eid),
                    data.type_name.clone(),
                    crate::causality::WriteKind::Spawn,
                    Self::component_summary(data),
                ));
            }
            target.add_components_bulk(eid, comps);
        }

        for rentry in body["resources"].as_array().into_iter().flatten() {
            let rpair = rentry
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or("load_world(): malformed resource entry")?;
            let rname = rpair[0]
                .as_str()
                .ok_or("load_world(): malformed resource entry")?;
            let vals = rpair[1]
                .as_array()
                .ok_or("load_world(): malformed resource values")?;
            let stored = schema
                .get(rname)
                .ok_or_else(|| format!("load_world(): no schema entry for '{}'", rname))?;
            let declared = self
                .world
                .get_resource(rname)
                .map(|d| d.layout)
                .ok_or_else(|| {
                    format!(
                        "load_world(): save contains resource '{}' which is not declared \
                         in this program",
                        rname
                    )
                })?;
            let data = match make_plan(stored, declared) {
                Plan::Direct { declared, map } => {
                    if vals.len() != map.len() {
                        return Err(format!(
                            "load_world(): resource '{}' has {} values, schema says {}",
                            rname,
                            vals.len(),
                            map.len()
                        ));
                    }
                    let mut values = Vec::with_capacity(map.len());
                    for si in map {
                        values.push(crate::wire::decode_value(&mut self.gc, &vals[si])?);
                    }
                    let data = crate::value::ComponentData {
                        type_name: rname.to_string(),
                        layout: declared,
                        values,
                    };
                    self.validate_loaded_row("load_world()", &data)?;
                    data
                }
                Plan::Migrate { stored, declared } => {
                    let from_version = *schema_versions.get(rname).unwrap_or(&0);
                    self.migrate_wire_row(rname, &stored, declared, vals, from_version)?
                }
            };
            let summary = Self::component_summary(&data);
            target.set_resource(rname, data);
            writes.push((
                None,
                rname.to_string(),
                crate::causality::WriteKind::Resource,
                summary,
            ));
        }

        self.world = target;
        for (entity, component, kind, summary) in writes {
            self.record_causal_write(entity, &component, kind, summary);
        }
        Ok(Value::from_int(&mut self.gc, loaded))
    }

    /// Run the declared `migrate` block for one row whose stored shape
    /// drifted from the declaration. Shared by `load_world` (v1 tagged saves)
    /// and `fork_from_bytes` (v2 wire payloads). `from_version` is the
    /// save's declared schema version for this type, bound to the optional
    /// second migrate parameter (dogfood seq 69 IDEA 03).
    fn migrate_loaded(
        &mut self,
        tname: &str,
        stored_pairs: Vec<(String, Value)>,
        declared: &std::sync::Arc<Vec<String>>,
        from_version: i64,
    ) -> Result<crate::value::ComponentData, String> {
        let Some(entry) = self.migrations.get(tname).copied() else {
            let stored_names: Vec<&String> = stored_pairs.iter().map(|(f, _)| f).collect();
            let added: Vec<&str> = declared
                .iter()
                .filter(|f| !stored_names.contains(f))
                .map(|f| f.as_str())
                .collect();
            let removed: Vec<&str> = stored_names
                .iter()
                .filter(|f| !declared.contains(f))
                .map(|f| f.as_str())
                .collect();
            return Err(format!(
                "load_world(): schema of '{}' changed (added: [{}], removed: [{}]) and no \
                 migration is declared â€” add `migrate {}(old) {{ return {} {{ ... }} }}`",
                tname,
                added.join(", "),
                removed.join(", "),
                tname,
                tname
            ));
        };

        let mut old_map = MapStorage::new();
        for (f, v) in stored_pairs {
            old_map.insert(MapKey::Str(f), v);
        }
        let old_value = Value::map(&mut self.gc, old_map);

        let result = self.run_migration_chunk(entry, old_value, from_version)?;
        let comp = result.as_component().ok_or_else(|| {
            format!(
                "migrate {}(old) must `return {} {{ ... }}`, got {}",
                tname,
                tname,
                result.type_name()
            )
        })?;
        if comp.type_name != tname {
            return Err(format!(
                "migrate {}(old) returned component '{}' â€” it must return '{}'",
                tname, comp.type_name, tname
            ));
        }
        // `old` binds persisted fields as map<str, any>, so the static
        // checker cannot see a wrong-typed migration result (the classic
        // mistake: grabbing the wrong old key). Enforce the declared field
        // types here, before the row becomes durable state.
        let ctx = format!("migrate {}(old)", tname);
        self.validate_loaded_row(&ctx, comp)?;
        Ok(comp.clone())
    }

    /// Invoke a compiled `migrate` chunk with the old-fields map (and the
    /// save's schema version, when the block declared a second parameter),
    /// returning the body's `return` value.
    fn run_migration_chunk(
        &mut self,
        entry: crate::vm::MigrationEntry,
        old_value: Value,
        from_version: i64,
    ) -> Result<Value, String> {
        let saved_depth = self.frames.len();
        let stack_base = self.stack.len();
        for _ in 0..entry.param_slot {
            self.push(Value::NIL);
        }
        self.push(old_value);
        if let Some(vslot) = entry.version_slot {
            // Pad any gap (defensive; in practice vslot == param_slot + 1),
            // then bind `from_version`.
            for _ in (entry.param_slot + 1)..vslot {
                self.push(Value::NIL);
            }
            let v = Value::from_int(&mut self.gc, from_version);
            self.push(v);
        }
        let frame_id = self.allocate_frame_id();
        self.frames.push(crate::vm::CallFrame {
            frame_id,
            chunk_id: entry.chunk_id,
            ip: 0,
            stack_base,
            captures: None,
            system_writeback: None,
        });
        // Migrations run mid-decode: the caller (fork_apply, fork_from_bytes,
        // load_world) holds already-decoded heap values in Rust locals the
        // collector cannot see. Auto-GC stays off for the duration.
        self.gc_pause += 1;
        let run = self
            .run_frames(saved_depth)
            .map_err(|error| error.to_string());
        self.gc_pause -= 1;
        run?;
        let result = self.pop()?;
        self.stack.truncate(stack_base);
        Ok(result)
    }

    /// `why(entity, Component) -> str` â€” causality query (#4): walks the
    /// provenance ledger from the last write to the component back through
    /// the handlerâ†’eventâ†’emitter chain.
    fn bi_why(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!("why() expects 2 arguments, got {}", args.len()));
        }
        let eid = args[0]
            .as_entity_id()
            .ok_or_else(|| format!("why() expects entity, got {}", args[0].type_name()))?;
        let ctype = Self::expect_component_type_name(&args[1], "why")?;
        // Provenance reveals a component's value history — a read, and a
        // richer one than `get`.
        self.sandbox_check_read(&ctype)?;
        let explanation = self.ledger.explain_entity(eid, &ctype, u64::MAX);
        Ok(Value::from_string(&mut self.gc, explanation))
    }

    /// `why_resource(Resource) -> str` â€” causality query for resources.
    fn bi_why_resource(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "why_resource() expects 1 argument, got {}",
                args.len()
            ));
        }
        let rtype = Self::expect_component_type_name(&args[0], "why_resource")?;
        self.sandbox_check_read(&rtype)?;
        let explanation = self.ledger.explain_resource(&rtype, u64::MAX);
        Ok(Value::from_string(&mut self.gc, explanation))
    }

    /// `diff(fork_a, fork_b) -> map<str, int>` â€” per-component changed-row
    /// counts between two forks (component/resource type name â†’ rows, an
    /// upper bound). O(archetypes) `Arc::ptr_eq` comparisons on CoW columns,
    /// not a world scan.
    fn bi_diff(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!("diff() expects 2 arguments, got {}", args.len()));
        }
        let summary = {
            let a = args[0]
                .as_world_fork()
                .ok_or_else(|| "diff() first argument must be a world_fork".to_string())?;
            let b = args[1]
                .as_world_fork()
                .ok_or_else(|| "diff() second argument must be a world_fork".to_string())?;
            crate::world::WorldSnapshot::diff_summary(a, b)
        };
        let mut m = MapStorage::new();
        for (name, count) in summary {
            let v = Value::from_int(&mut self.gc, count as i64);
            m.insert(MapKey::Str(name), v);
        }
        Ok(Value::map(&mut self.gc, m))
    }

    /// `assert_only_changed(fork_a, fork_b, allowed)` â€” the negative-space
    /// assertion: errors unless every difference between the two forks is in
    /// the `allowed` component list (component type refs or name strings).
    ///
    /// This is only possible because the language owns 100% of program state:
    /// "nothing else in the universe changed" is a checkable sentence.
    fn bi_assert_only_changed(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "assert_only_changed() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let allowed: std::collections::HashSet<String> = args[2]
            .as_list()
            .ok_or_else(|| {
                "assert_only_changed() third argument must be a list of component types".to_string()
            })?
            .iter()
            .map(|v| Self::expect_component_type_name(v, "assert_only_changed"))
            .collect::<Result<_, _>>()?;
        let summary = {
            let a = args[0].as_world_fork().ok_or_else(|| {
                "assert_only_changed() first argument must be a world_fork".to_string()
            })?;
            let b = args[1].as_world_fork().ok_or_else(|| {
                "assert_only_changed() second argument must be a world_fork".to_string()
            })?;
            crate::world::WorldSnapshot::diff_summary(a, b)
        };
        let unexpected: Vec<String> = summary
            .iter()
            .filter(|(name, _)| !allowed.contains(*name))
            .map(|(name, rows)| format!("{} ({} rows)", name, rows))
            .collect();
        if !unexpected.is_empty() {
            let mut allowed_sorted: Vec<&String> = allowed.iter().collect();
            allowed_sorted.sort();
            return Err(format!(
                "assert_only_changed() failed: unexpected changes to [{}] (allowed: [{}])",
                unexpected.join(", "),
                allowed_sorted
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(Value::NIL)
    }

    /// `sandbox_input() -> any` â€” the data-only input handed to this guest by
    /// the host (`nil` when none was provided or outside a sandbox).
    fn bi_sandbox_input(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "sandbox_input() takes no arguments, got {}",
                args.len()
            ));
        }
        match self.sandbox_input_json.take() {
            Some(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| format!("sandbox_input(): host sent invalid JSON: {}", e))?;
                let v = json_to_value(&mut self.gc, &parsed)?;
                // Re-arm so repeated calls keep working.
                self.sandbox_input_json = Some(text);
                Ok(v)
            }
            None => Ok(Value::NIL),
        }
    }

    /// `sandbox_output(v)` â€” report a structured, data-only result to the
    /// host. The value is serialized to JSON immediately, so nothing from the
    /// guest heap survives past the guest VM. Last call wins.
    fn bi_sandbox_output(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "sandbox_output() expects 1 argument, got {}",
                args.len()
            ));
        }
        let j = value_to_json(&args[0], 0)
            .map_err(|e| format!("sandbox_output() value is not data-only: {}", e))?;
        self.sandbox_output_json = Some(j.to_string());
        Ok(Value::NIL)
    }

    /// `sandbox_last_output() -> any | nil` — the structured value the most
    /// recent `sandbox_run` guest reported via `sandbox_output(v)`, parsed
    /// back onto the host heap (the same data-only JSON boundary as
    /// `sandbox_input`, in reverse), or `nil` if the guest emitted none. This
    /// closes the in-language gap where `run_sandbox_guest` computed the
    /// guest's output and `sandbox_run` discarded it (dogfood feature seq 62),
    /// leaving a Rad host unable to read a plugin's typed result without
    /// forcing it to WRITE state just to communicate.
    fn bi_sandbox_last_output(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "sandbox_last_output() takes no arguments, got {}",
                args.len()
            ));
        }
        match &self.last_sandbox_output_json {
            Some(text) => {
                let parsed: serde_json::Value = serde_json::from_str(text).map_err(|e| {
                    format!(
                        "sandbox_last_output(): stored guest output is invalid JSON: {}",
                        e
                    )
                })?;
                json_to_value(&mut self.gc, &parsed)
            }
            None => Ok(Value::NIL),
        }
    }

    /// `sandbox_last_fuel() -> int` — fuel consumed by the most recent
    /// `sandbox_run` (charge points crossed: loop back-edges and calls), or 0
    /// before any run. The metering signal a plugin host bills or rate-limits
    /// on; also computed-then-discarded before seq 62.
    fn bi_sandbox_last_fuel(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "sandbox_last_fuel() takes no arguments, got {}",
                args.len()
            ));
        }
        Ok(Value::from_int(
            &mut self.gc,
            self.last_sandbox_fuel_spent as i64,
        ))
    }
}

pub(crate) fn bi_len(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("len() requires 1 argument".into());
    }
    if let Some(items) = args[0].as_list() {
        Ok(Value::from_int(gc, items.len() as i64))
    } else if let Some(t) = args[0].as_tuple() {
        Ok(Value::from_int(gc, t.len() as i64))
    } else if let Some(s) = args[0].as_str() {
        Ok(Value::from_int(gc, s.chars().count() as i64))
    } else if let Some(m) = args[0].as_map() {
        Ok(Value::from_int(gc, m.len() as i64))
    } else if let Some(bytes) = args[0].as_bytebuf() {
        Ok(Value::from_int(gc, bytes.len() as i64))
    } else {
        Err(format!("len() not defined for {}", args[0].type_name()))
    }
}

pub(crate) fn bi_typeof(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("typeof() requires 1 argument".into());
    }
    Ok(Value::from_string(gc, args[0].type_name().to_string()))
}

pub(crate) fn bi_str(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("str() requires 1 argument".into());
    }
    Ok(Value::from_string(gc, args[0].print_display()))
}

pub(crate) fn bi_int(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("int() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        Ok(Value::from_int(gc, n))
    } else if let Some(x) = args[0].as_float() {
        if x.is_nan() {
            return Err("Cannot convert NaN to int".into());
        }
        if x.is_infinite() || x > i64::MAX as f64 || x < i64::MIN as f64 {
            return Err(format!(
                "Cannot convert {} to int: value out of i64 range",
                x
            ));
        }
        Ok(Value::from_int(gc, x as i64))
    } else if let Some(s) = args[0].as_str() {
        s.parse::<i64>()
            .map(|n| Value::from_int(gc, n))
            .map_err(|_| format!("Cannot convert '{}' to int", s))
    } else if let Some(b) = args[0].as_bool() {
        Ok(Value::from_int(gc, if b { 1 } else { 0 }))
    } else {
        Err(format!("Cannot convert {} to int", args[0].type_name()))
    }
}

pub(crate) fn bi_float(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("float() requires 1 argument".into());
    }
    if let Some(x) = args[0].as_float() {
        Ok(Value::from_float(x))
    } else if let Some(n) = args[0].as_int() {
        Ok(Value::from_float(n as f64))
    } else if let Some(s) = args[0].as_str() {
        s.parse::<f64>()
            .map(Value::from_float)
            .map_err(|_| format!("Cannot convert '{}' to float", s))
    } else {
        Err(format!("Cannot convert {} to float", args[0].type_name()))
    }
}

pub(crate) fn bi_int_div(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("int_div() requires exactly 2 arguments".into());
    }
    let a = args[0].as_int().ok_or_else(|| {
        format!(
            "int_div() first argument must be int, got {}",
            args[0].type_name()
        )
    })?;
    let b = args[1].as_int().ok_or_else(|| {
        format!(
            "int_div() second argument must be int, got {}",
            args[1].type_name()
        )
    })?;
    if b == 0 {
        return Err("Division by zero".into());
    }
    let result = a
        .checked_div(b)
        .ok_or_else(|| format!("Integer overflow: {} / {}", a, b))?;
    Ok(Value::from_int(gc, result))
}

pub(crate) fn bi_abs(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("abs() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        let result = n
            .checked_abs()
            .ok_or_else(|| format!("Integer overflow: abs({})", n))?;
        Ok(Value::from_int(gc, result))
    } else if let Some(x) = args[0].as_float() {
        Ok(Value::from_float(x.abs()))
    } else {
        Err(format!("abs() not defined for {}", args[0].type_name()))
    }
}

pub(crate) fn bi_sign(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("sign() requires 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        Ok(Value::from_int(gc, n.signum()))
    } else if let Some(x) = args[0].as_float() {
        // Math.sign semantics: 0.0 and NaN map to 0.0, not ±1
        let s = if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        };
        Ok(Value::from_float(s))
    } else {
        Err(format!("sign() not defined for {}", args[0].type_name()))
    }
}

fn int_arg(args: &[Value], idx: usize, fname: &str) -> Result<i64, String> {
    let v = args
        .get(idx)
        .ok_or_else(|| format!("{}() missing argument {}", fname, idx + 1))?;
    v.as_int()
        .ok_or_else(|| format!("{}() expects an int, got {}", fname, v.type_name()))
}

/// `popcount(x) -> int` — number of set bits (bitboard workloads).
pub(crate) fn bi_popcount(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let n = int_arg(&args, 0, "popcount")?;
    Ok(Value::from_int(gc, n.count_ones() as i64))
}

/// `ctz(x) -> int` — index of the lowest set bit (64 when x == 0).
pub(crate) fn bi_ctz(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let n = int_arg(&args, 0, "ctz")?;
    Ok(Value::from_int(gc, n.trailing_zeros() as i64))
}

/// `shl(x, n) -> int` — logical shift left; n outside 0..63 returns 0.
pub(crate) fn bi_shl(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let x = int_arg(&args, 0, "shl")?;
    let n = int_arg(&args, 1, "shl")?;
    let out = if !(0..64).contains(&n) {
        0
    } else {
        ((x as u64) << n) as i64
    };
    Ok(Value::from_int(gc, out))
}

/// `filled(n, v) -> list` — a list of `n` copies of `v`, built natively.
/// The interpreted equivalent (`for _ in range(n) { xs << v }`) pays the
/// dispatch loop per element — a real tax on solver-style scratch buffers.
pub(crate) fn bi_filled(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let n = int_arg(&args, 0, "filled")?;
    if n < 0 {
        return Err(format!("filled() length must be non-negative, got {}", n));
    }
    let v = *args
        .get(1)
        .ok_or_else(|| "filled() missing argument 2".to_string())?;
    let items = vec![v; n as usize];
    Ok(Value::from_rad_list(gc, crate::value::RadList::new(items)))
}

/// `shr(x, n) -> int` — logical shift right; n outside 0..63 returns 0.
pub(crate) fn bi_shr(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    let x = int_arg(&args, 0, "shr")?;
    let n = int_arg(&args, 1, "shr")?;
    let out = if !(0..64).contains(&n) {
        0
    } else {
        ((x as u64) >> n) as i64
    };
    Ok(Value::from_int(gc, out))
}

fn number_arg(args: &[Value], idx: usize, fname: &str) -> Result<f64, String> {
    let v = args
        .get(idx)
        .ok_or_else(|| format!("{}() missing argument {}", fname, idx + 1))?;
    if let Some(n) = v.as_int() {
        Ok(n as f64)
    } else if let Some(x) = v.as_float() {
        Ok(x)
    } else {
        Err(format!(
            "{}() expects a number, got {}",
            fname,
            v.type_name()
        ))
    }
}

fn float_to_int_result(gc: &mut GcHeap, r: f64, fname: &str) -> Result<Value, String> {
    if !r.is_finite() {
        return Err(format!("{}() result is not finite", fname));
    }
    if r < i64::MIN as f64 || r > i64::MAX as f64 {
        return Err(format!("{}() result out of int range: {}", fname, r));
    }
    Ok(Value::from_int(gc, r as i64))
}

pub(crate) fn bi_round(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("round() requires exactly 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        return Ok(Value::from_int(gc, n));
    }
    // f64::round = half away from zero (correct for -0.5 cases, unlike int(x + 0.5))
    let x = number_arg(&args, 0, "round")?;
    float_to_int_result(gc, x.round(), "round")
}

pub(crate) fn bi_floor(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("floor() requires exactly 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        return Ok(Value::from_int(gc, n));
    }
    let x = number_arg(&args, 0, "floor")?;
    float_to_int_result(gc, x.floor(), "floor")
}

pub(crate) fn bi_ceil(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("ceil() requires exactly 1 argument".into());
    }
    if let Some(n) = args[0].as_int() {
        return Ok(Value::from_int(gc, n));
    }
    let x = number_arg(&args, 0, "ceil")?;
    float_to_int_result(gc, x.ceil(), "ceil")
}

pub(crate) fn bi_sqrt(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("sqrt() requires exactly 1 argument".into());
    }
    let x = number_arg(&args, 0, "sqrt")?;
    if x < 0.0 {
        return Err(format!("sqrt() of negative number: {}", x));
    }
    Ok(Value::from_float(x.sqrt()))
}

pub(crate) fn bi_pow(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("pow() requires exactly 2 arguments".into());
    }
    // int ^ non-negative int stays an int (with overflow check)
    if let (Some(base), Some(exp)) = (args[0].as_int(), args[1].as_int()) {
        if exp >= 0 {
            let exp_u32 = u32::try_from(exp)
                .map_err(|_| format!("pow() integer exponent too large: {}", exp))?;
            let result = base
                .checked_pow(exp_u32)
                .ok_or_else(|| format!("Integer overflow: pow({}, {})", base, exp))?;
            return Ok(Value::from_int(gc, result));
        }
    }
    let base = number_arg(&args, 0, "pow")?;
    let exp = number_arg(&args, 1, "pow")?;
    Ok(Value::from_float(base.powf(exp)))
}

pub(crate) fn bi_to_fixed(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("to_fixed() requires exactly 2 arguments".into());
    }
    let x = number_arg(&args, 0, "to_fixed")?;
    let digits = args[1]
        .as_int()
        .ok_or_else(|| format!("to_fixed() digits must be int, got {}", args[1].type_name()))?;
    if !(0..=17).contains(&digits) {
        return Err(format!("to_fixed() digits must be 0..=17, got {}", digits));
    }
    Ok(Value::from_string(gc, format!("{:.*}", digits as usize, x)))
}

const JSON_MAX_DEPTH: usize = 128;

pub(crate) fn value_to_json(v: &Value, depth: usize) -> Result<serde_json::Value, String> {
    if depth > JSON_MAX_DEPTH {
        return Err("json_stringify() exceeded max nesting depth".into());
    }
    if v.is_nil() {
        return Ok(serde_json::Value::Null);
    }
    if let Some(b) = v.as_bool() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Some(n) = v.as_int() {
        return Ok(serde_json::Value::Number(n.into()));
    }
    if let Some(x) = v.as_float() {
        return serde_json::Number::from_f64(x)
            .map(serde_json::Value::Number)
            .ok_or_else(|| format!("json_stringify() cannot encode non-finite float: {}", x));
    }
    if let Some(s) = v.as_str() {
        return Ok(serde_json::Value::String(s.to_string()));
    }
    if let Some(items) = v.as_list() {
        let mut arr = Vec::with_capacity(items.len());
        for item in items.iter() {
            arr.push(value_to_json(item, depth + 1)?);
        }
        return Ok(serde_json::Value::Array(arr));
    }
    if let Some(m) = v.as_map() {
        let mut obj = serde_json::Map::with_capacity(m.len());
        let mut sorted_keys: Vec<&MapKey> = m.keys().collect();
        sorted_keys.sort();
        for k in sorted_keys {
            let key_str = match k {
                MapKey::Str(s) => s.clone(),
                MapKey::Int(i) => i.to_string(),
                MapKey::Bool(b) => b.to_string(),
                MapKey::Entity(e) => e.to_string(),
                // JSON object keys must be strings: "(1, 2)"
                MapKey::Tuple(_) => k.to_string(),
            };
            obj.insert(key_str, value_to_json(&m[k], depth + 1)?);
        }
        return Ok(serde_json::Value::Object(obj));
    }
    if let Some(c) = v.as_component() {
        let mut obj = serde_json::Map::with_capacity(c.layout.len());
        for (idx, field) in c.layout.iter().enumerate() {
            obj.insert(field.clone(), value_to_json(&c.values[idx], depth + 1)?);
        }
        return Ok(serde_json::Value::Object(obj));
    }
    if let Some(st) = v.as_sum_type() {
        let mut obj = serde_json::Map::with_capacity(st.fields.len() + 1);
        obj.insert(
            "$variant".to_string(),
            serde_json::Value::String(st.variant.clone()),
        );
        let mut keys: Vec<&String> = st.fields.keys().collect();
        keys.sort();
        for k in keys {
            obj.insert(k.clone(), value_to_json(&st.fields[k], depth + 1)?);
        }
        return Ok(serde_json::Value::Object(obj));
    }
    Err(format!("json_stringify() cannot encode {}", v.type_name()))
}

pub(crate) fn json_to_value(gc: &mut GcHeap, j: &serde_json::Value) -> Result<Value, String> {
    match j {
        serde_json::Value::Null => Ok(Value::NIL),
        serde_json::Value::Bool(b) => Ok(Value::from_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::from_int(gc, i))
            } else if let Some(x) = n.as_f64() {
                Ok(Value::from_float(x))
            } else {
                Err(format!("json_parse() unsupported number: {}", n))
            }
        }
        serde_json::Value::String(s) => Ok(Value::from_string(gc, s.clone())),
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_value(gc, item)?);
            }
            Ok(Value::list(gc, out))
        }
        serde_json::Value::Object(obj) => {
            let mut m = MapStorage::new();
            for (k, v) in obj {
                let val = json_to_value(gc, v)?;
                m.insert(MapKey::Str(k.clone()), val);
            }
            Ok(Value::map(gc, m))
        }
    }
}

pub(crate) fn bi_json_stringify(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json_stringify() requires exactly 1 argument".into());
    }
    let json = value_to_json(&args[0], 0)?;
    let text =
        serde_json::to_string(&json).map_err(|e| format!("json_stringify() failed: {}", e))?;
    Ok(Value::from_string(gc, text))
}

pub(crate) fn bi_json_parse(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("json_parse() requires exactly 1 argument".into());
    }
    let text = args[0]
        .as_str()
        .ok_or_else(|| format!("json_parse() expects str, got {}", args[0].type_name()))?;
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
    match parsed {
        Ok(j) => {
            let v = json_to_value(gc, &j)?;
            Ok(wrap_option(gc, Some(v)))
        }
        Err(_) => Ok(wrap_option(gc, None)),
    }
}

pub(crate) fn bi_min(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("min() requires 2 arguments".into());
    }
    match (
        args[0].as_int(),
        args[0].as_float(),
        args[1].as_int(),
        args[1].as_float(),
    ) {
        (Some(a), _, Some(b), _) => Ok(Value::from_int(gc, a.min(b))),
        (_, Some(a), _, Some(b)) => Ok(Value::from_float(a.min(b))),
        (Some(a), _, _, Some(b)) => Ok(Value::from_float((a as f64).min(b))),
        (_, Some(a), Some(b), _) => Ok(Value::from_float(a.min(b as f64))),
        _ => Err(format!(
            "min() not defined for {} and {}",
            args[0].type_name(),
            args[1].type_name()
        )),
    }
}

pub(crate) fn bi_max(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("max() requires 2 arguments".into());
    }
    match (
        args[0].as_int(),
        args[0].as_float(),
        args[1].as_int(),
        args[1].as_float(),
    ) {
        (Some(a), _, Some(b), _) => Ok(Value::from_int(gc, a.max(b))),
        (_, Some(a), _, Some(b)) => Ok(Value::from_float(a.max(b))),
        (Some(a), _, _, Some(b)) => Ok(Value::from_float((a as f64).max(b))),
        (_, Some(a), Some(b), _) => Ok(Value::from_float(a.max(b as f64))),
        _ => Err(format!(
            "max() not defined for {} and {}",
            args[0].type_name(),
            args[1].type_name()
        )),
    }
}

pub(crate) fn bi_unwrap(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("unwrap() requires 1 argument".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" && st.variant == "Some" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Option" && st.variant == "None" {
            // unwrap() erases all context by design — teach the tools that
            // keep it: expect() for your own words, require() for component
            // reads (it names the entity and what it has), unwrap_or() when
            // a default is fine.
            return Err(
                "unwrap() called on Option::None\n  hint: expect(value, \"why\") attaches your own message; \
                 require(entity, Comp) names the entity and component when a read fails; \
                 unwrap_or(value, default) when missing is fine"
                    .to_string(),
            );
        }
        if st.type_name == "Result" && st.variant == "Ok" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Result" && st.variant == "Err" {
            let msg = st
                .fields
                .get("message")
                .map(|v| v.print_display())
                .unwrap_or_default();
            return Err(format!(
                "unwrap() called on Result::Err: {}\n  hint: match on Ok/Err to handle the failure, or expect(value, \"why\") to rename it",
                msg
            ));
        }
    }
    Ok(args[0])
}

pub(crate) fn bi_expect(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("expect() requires 2 arguments".into());
    }
    let msg = args[1].print_display();
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" && st.variant == "Some" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Option" && st.variant == "None" {
            return Err(format!("expect() failed: {}", msg));
        }
        if st.type_name == "Result" && st.variant == "Ok" {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if st.type_name == "Result" && st.variant == "Err" {
            return Err(format!("expect() failed: {}", msg));
        }
    }
    Ok(args[0])
}

pub(crate) fn bi_unwrap_or(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("unwrap_or() requires 2 arguments (option_or_result, default)".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if (st.type_name == "Option" && st.variant == "Some")
            || (st.type_name == "Result" && st.variant == "Ok")
        {
            return Ok(st.fields.get("value").copied().unwrap_or(Value::NIL));
        }
        if (st.type_name == "Option" && st.variant == "None")
            || (st.type_name == "Result" && st.variant == "Err")
        {
            return Ok(args[1]);
        }
    }
    Ok(args[0])
}

pub(crate) fn bi_is_some(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("is_some() requires 1 argument".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" {
            return Ok(Value::from_bool(st.variant == "Some"));
        }
        if st.type_name == "Result" {
            return Ok(Value::from_bool(st.variant == "Ok"));
        }
    }
    Ok(Value::from_bool(false))
}

pub(crate) fn bi_is_none(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("is_none() requires 1 argument".into());
    }
    if let Some(st) = args[0].as_sum_type() {
        if st.type_name == "Option" {
            return Ok(Value::from_bool(st.variant == "None"));
        }
        if st.type_name == "Result" {
            return Ok(Value::from_bool(st.variant == "Err"));
        }
    }
    Ok(Value::from_bool(false))
}

/// `set_at(coll, key, v) -> list|map` — a copy of `coll` with `key`
/// replaced by `v` (CoW: cheap when uniquely owned). The expression
/// dual of the `coll[key] = v` statement and the lowering target for
/// indexed field updates in `update` blocks. Lists bounds-check (no
/// silent growth); maps insert-or-replace, exactly like `m[k] = v`.
pub(crate) fn bi_set_at(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("set_at() requires 3 arguments (list-or-map, key, value)".into());
    }
    let mut arg_iter = args.into_iter();
    let collection = arg_iter.next().unwrap();
    let idx = arg_iter.next().unwrap();
    let value = arg_iter.next().unwrap();
    let got = collection.type_name();
    if collection.as_list().is_some() {
        let Some(i) = idx.as_int() else {
            return Err(format!(
                "set_at() list index must be int, got {}",
                idx.type_name()
            ));
        };
        let mut items = collection.into_rad_list().unwrap();
        let len = items.len() as i64;
        if i < 0 || i >= len {
            return Err(format!(
                "set_at() index {} out of bounds for list of length {}",
                i, len
            ));
        }
        items.set(i as usize, value)?;
        Ok(Value::from_rad_list(gc, items))
    } else if collection.as_map().is_some() {
        let map_key = crate::value::MapKey::from_value(&idx)?;
        let mut new_map = collection.into_map().unwrap();
        new_map.insert(map_key, value);
        Ok(Value::map(gc, new_map))
    } else {
        Err(format!("set_at() expects a list or map, got {}", got))
    }
}

/// `sum(xs)` / `product(xs)` — numeric folds, the missing halves of every
/// stat pipeline (`mods |> map(.flat) |> sum`). Ints stay ints; any float
/// in the list promotes the result. Empty list: sum 0, product 1.
fn numeric_fold(
    gc: &mut GcHeap,
    args: Vec<Value>,
    name: &str,
    int_init: i64,
    int_op: fn(i64, i64) -> i64,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!(
            "{}() requires 1 argument (a list of numbers)",
            name
        ));
    }
    let Some(items) = args[0].as_list() else {
        return Err(format!(
            "{}() expects a list, got {}",
            name,
            args[0].type_name()
        ));
    };
    let mut acc_i = int_init;
    let mut acc_f = int_init as f64;
    let mut is_float = false;
    for v in items.iter() {
        if let Some(i) = v.as_int() {
            acc_i = int_op(acc_i, i);
            acc_f = float_op(acc_f, i as f64);
        } else if let Some(f) = v.as_float() {
            is_float = true;
            acc_f = float_op(acc_f, f);
        } else {
            return Err(format!(
                "{}() expects numeric elements, got {}",
                name,
                v.type_name()
            ));
        }
    }
    if is_float {
        Ok(Value::from_float(acc_f))
    } else {
        Ok(Value::from_int(gc, acc_i))
    }
}

/// `get_or(coll, key, default)` — map lookup or list index with a fallback
/// instead of nil/bounds-error. The shape of every cooldown/stat table read.
pub(crate) fn bi_get_or(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("get_or() requires 3 arguments (collection, key, default)".into());
    }
    let coll = &args[0];
    let key = &args[1];
    let default = args[2];
    if let Some(m) = coll.as_map() {
        let map_key = crate::value::MapKey::from_value(key)?;
        return Ok(m.get(&map_key).copied().unwrap_or(default));
    }
    if let Some(xs) = coll.as_list() {
        let Some(i) = key.as_int() else {
            return Err(format!(
                "get_or() list index must be int, got {}",
                key.type_name()
            ));
        };
        if i < 0 || i as usize >= xs.len() {
            return Ok(default);
        }
        return Ok(*xs.get(i as usize).unwrap_or(&default));
    }
    Err(format!(
        "get_or() expects a map or list, got {}",
        coll.type_name()
    ))
}

/// `index_of(xs, v) -> int` — first index holding `v`, or -1. Returns an
/// int (not an Option) because the consumer is slot arithmetic
/// (`if at >= 0 { set_at(slots, at, ...) }`), and -1 composes with it.
pub(crate) fn bi_index_of(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("index_of() requires 2 arguments (list, value)".into());
    }
    let Some(xs) = args[0].as_list() else {
        return Err(format!(
            "index_of() expects a list, got {}",
            args[0].type_name()
        ));
    };
    for (i, v) in xs.iter().enumerate() {
        if helpers::values_equal(v, &args[1]) {
            return Ok(Value::from_int(gc, i as i64));
        }
    }
    Ok(Value::from_int(gc, -1))
}

/// `clamp(x, lo, hi)` — pin a number to a range. Ints stay int when all
/// three are ints; any float promotes.
pub(crate) fn bi_clamp(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("clamp() requires 3 arguments (value, lo, hi)".into());
    }
    if let (Some(x), Some(lo), Some(hi)) = (args[0].as_int(), args[1].as_int(), args[2].as_int()) {
        if lo > hi {
            return Err(format!("clamp() lo {} exceeds hi {}", lo, hi));
        }
        return Ok(Value::from_int(gc, x.max(lo).min(hi)));
    }
    let as_f = |v: &Value| v.as_int().map(|i| i as f64).or(v.as_float());
    if let (Some(x), Some(lo), Some(hi)) = (as_f(&args[0]), as_f(&args[1]), as_f(&args[2])) {
        if lo > hi {
            return Err(format!("clamp() lo {} exceeds hi {}", lo, hi));
        }
        return Ok(Value::from_float(x.max(lo).min(hi)));
    }
    Err(format!(
        "clamp() expects numbers, got {}, {}, {}",
        args[0].type_name(),
        args[1].type_name(),
        args[2].type_name()
    ))
}

pub(crate) fn bi_sum(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    numeric_fold(gc, args, "sum", 0, |a, b| a.wrapping_add(b), |a, b| a + b)
}

pub(crate) fn bi_product(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    numeric_fold(
        gc,
        args,
        "product",
        1,
        |a, b| a.wrapping_mul(b),
        |a, b| a * b,
    )
}

pub(crate) fn bi_push(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("push() requires 2 arguments".into());
    }
    let mut arg_iter = args.into_iter();
    let collection = arg_iter.next().unwrap();
    let item = arg_iter.next().unwrap();
    let got = collection.type_name();

    if collection.as_list().is_some() {
        let mut items = collection.into_rad_list().unwrap();
        items.push(item);
        Ok(Value::from_rad_list(gc, items))
    } else if collection.as_str().is_some() {
        let mut s = collection.into_string().unwrap();
        if let Some(item_str) = item.as_str() {
            s.push_str(item_str);
            Ok(Value::from_string(gc, s))
        } else {
            Err(format!(
                "push() on string expects string argument, got {}",
                item.type_name()
            ))
        }
    } else {
        Err(format!("push() expects list or string, got {}", got))
    }
}

pub(crate) fn bi_pop(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("pop() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    if let Some(items) = arg.as_list() {
        if items.is_empty() {
            return Err("pop() on empty list".to_string());
        }
        Ok(*items.last().unwrap())
    } else if let Some(s) = arg.as_str() {
        if s.is_empty() {
            return Err("pop() on empty string".to_string());
        }
        Ok(Value::from_string(
            gc,
            s.chars().last().unwrap().to_string(),
        ))
    } else {
        Err(format!(
            "pop() expects list or string, got {}",
            arg.type_name()
        ))
    }
}

pub(crate) fn bi_pop_last(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("pop_last() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    if let Some(items) = arg.as_list() {
        if items.is_empty() {
            return Err("pop_last() on empty list".to_string());
        }
        Ok(*items.last().unwrap())
    } else if let Some(s) = arg.as_str() {
        if s.is_empty() {
            return Err("pop_last() on empty string".to_string());
        }
        Ok(Value::from_string(
            gc,
            s.chars().last().unwrap().to_string(),
        ))
    } else {
        Err(format!(
            "pop_last() expects list or string, got {}",
            arg.type_name()
        ))
    }
}

pub(crate) fn bi_drop_last(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("drop_last() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let got = arg.type_name().to_string();

    if arg.as_list().is_some() {
        let mut items = arg.into_rad_list().unwrap();
        if items.is_empty() {
            return Err("drop_last() on empty list".to_string());
        }
        items.pop();
        Ok(Value::from_rad_list(gc, items))
    } else if arg.as_str().is_some() {
        let mut s = arg.into_string().unwrap();
        if s.is_empty() {
            return Err("drop_last() on empty string".to_string());
        }
        s.pop();
        Ok(Value::from_string(gc, s))
    } else {
        Err(format!("drop_last() expects list or string, got {}", got))
    }
}

/// `drop_first(xs)` — the queue-advance dual of drop_last: everything
/// after the head. Errors on empty (no silent no-op on a drained queue).
pub(crate) fn bi_drop_first(gc: &mut GcHeap, mut args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("drop_first() requires 1 argument".into());
    }
    let arg = args.pop().unwrap();
    let got = arg.type_name().to_string();
    if arg.as_list().is_some() {
        let items = arg.into_rad_list().unwrap();
        if items.is_empty() {
            return Err("drop_first() on empty list".to_string());
        }
        Ok(Value::from_rad_list(
            gc,
            crate::value::RadList::new(items.as_slice()[1..].to_vec()),
        ))
    } else if arg.as_str().is_some() {
        let s = arg.as_str().unwrap();
        if s.is_empty() {
            return Err("drop_first() on empty string".to_string());
        }
        let mut chars = s.chars();
        chars.next();
        Ok(Value::from_string(gc, chars.as_str().to_string()))
    } else {
        Err(format!("drop_first() expects list or string, got {}", got))
    }
}

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
    if args.is_empty() {
        return Err("range() requires at least 1 argument".into());
    }
    let (start, end, step) = match args.len() {
        1 => (0i64, args[0].as_int().ok_or("range() expects int")?, 1i64),
        2 => (
            args[0].as_int().ok_or("range() expects int")?,
            args[1].as_int().ok_or("range() expects int")?,
            1i64,
        ),
        _ => (
            args[0].as_int().ok_or("range() expects int")?,
            args[1].as_int().ok_or("range() expects int")?,
            args[2].as_int().ok_or("range() expects int")?,
        ),
    };
    if step == 0 {
        return Err("range() step cannot be zero".into());
    }
    let mut result = Vec::new();
    let mut i = start;
    if step > 0 {
        while i < end {
            result.push(Value::from_int(gc, i));
            i += step;
        }
    } else {
        while i > end {
            result.push(Value::from_int(gc, i));
            i += step;
        }
    }
    Ok(Value::list(gc, result))
}

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

pub(crate) fn bi_split(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("split() requires 2 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("split() expects string, got {}", args[0].type_name()))?;
    let delim = args[1].as_str().ok_or_else(|| {
        format!(
            "split() delimiter must be string, got {}",
            args[1].type_name()
        )
    })?;

    let parts: Vec<Value> = if delim.is_empty() {
        s.chars()
            .map(|p| Value::from_string(gc, p.to_string()))
            .collect()
    } else {
        s.split(delim)
            .map(|p| Value::from_string(gc, p.to_string()))
            .collect()
    };
    Ok(Value::list(gc, parts))
}

pub(crate) fn bi_join(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("join() requires 2 arguments".into());
    }
    let items = args[0]
        .as_list()
        .ok_or_else(|| format!("join() expects list, got {}", args[0].type_name()))?;
    let sep = args[1].as_str().ok_or_else(|| {
        format!(
            "join() separator must be string, got {}",
            args[1].type_name()
        )
    })?;
    let strs: Vec<String> = items.iter().map(|v| v.print_display()).collect();
    Ok(Value::from_string(gc, strs.join(sep)))
}

pub(crate) fn bi_trim(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("trim() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("trim() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_string(gc, s.trim().to_string()))
}

pub(crate) fn bi_replace(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 3 {
        return Err("replace() requires 3 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("replace() expects string, got {}", args[0].type_name()))?;
    let from = args[1].as_str().ok_or_else(|| {
        format!(
            "replace() pattern must be string, got {}",
            args[1].type_name()
        )
    })?;
    let to = args[2].as_str().ok_or_else(|| {
        format!(
            "replace() replacement must be string, got {}",
            args[2].type_name()
        )
    })?;
    Ok(Value::from_string(gc, s.replace(from, to)))
}

pub(crate) fn bi_starts_with(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("starts_with() requires 2 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("starts_with() expects string, got {}", args[0].type_name()))?;
    let prefix = args[1].as_str().ok_or_else(|| {
        format!(
            "starts_with() prefix must be string, got {}",
            args[1].type_name()
        )
    })?;
    Ok(Value::from_bool(s.starts_with(prefix)))
}

pub(crate) fn bi_ends_with(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("ends_with() requires 2 arguments".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("ends_with() expects string, got {}", args[0].type_name()))?;
    let suffix = args[1].as_str().ok_or_else(|| {
        format!(
            "ends_with() suffix must be string, got {}",
            args[1].type_name()
        )
    })?;
    Ok(Value::from_bool(s.ends_with(suffix)))
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

pub(crate) fn bi_try_int(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("try_int() requires 1 argument".into());
    }
    let result = if let Some(n) = args[0].as_int() {
        Some(Value::from_int(gc, n))
    } else if let Some(x) = args[0].as_float() {
        Some(Value::from_int(gc, x as i64))
    } else if let Some(s) = args[0].as_str() {
        s.parse::<i64>().ok().map(|n| Value::from_int(gc, n))
    } else {
        args[0]
            .as_bool()
            .map(|b| Value::from_int(gc, if b { 1 } else { 0 }))
    };
    Ok(wrap_option(gc, result))
}

pub(crate) fn bi_try_float(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("try_float() requires 1 argument".into());
    }
    let result = if let Some(x) = args[0].as_float() {
        Some(Value::from_float(x))
    } else if let Some(n) = args[0].as_int() {
        Some(Value::from_float(n as f64))
    } else if let Some(s) = args[0].as_str() {
        s.parse::<f64>().ok().map(Value::from_float)
    } else {
        None
    };
    Ok(wrap_option(gc, result))
}

pub(crate) fn wrap_option(gc: &mut GcHeap, val: Option<Value>) -> Value {
    match val {
        Some(v) => {
            let mut fields = HashMap::new();
            fields.insert("value".to_string(), v);
            Value::sum_type(gc, "Option".to_string(), "Some".to_string(), fields)
        }
        None => Value::sum_type(gc, "Option".to_string(), "None".to_string(), HashMap::new()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn tcp_accept_with_timeout(
    listener: &std::net::TcpListener,
    timeout_ms: i64,
) -> Result<Option<std::net::TcpStream>, String> {
    listener.set_nonblocking(true).map_err(|e| {
        format!(
            "tcp_accept_timeout() failed to enter nonblocking mode: {}",
            e
        )
    })?;
    let result = tcp_accept_nonblocking_loop(listener, timeout_ms);
    if let Err(e) = listener.set_nonblocking(false) {
        return Err(format!(
            "tcp_accept_timeout() failed to restore blocking mode: {}",
            e
        ));
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn tcp_accept_nonblocking_loop(
    listener: &std::net::TcpListener,
    timeout_ms: i64,
) -> Result<Option<std::net::TcpStream>, String> {
    let deadline = poll_deadline(timeout_ms, "tcp_accept_timeout()")?;
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => return Ok(Some(stream)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !sleep_until_next_poll(deadline) {
                    return Ok(None);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("tcp_accept_timeout() failed: {}", e)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct UdpPacket {
    data: Vec<u8>,
    addr: std::net::SocketAddr,
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_from_blocking(
    socket: &std::net::UdpSocket,
    max_bytes: usize,
) -> Result<UdpPacket, String> {
    let mut buf = vec![0u8; max_bytes];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                buf.truncate(n);
                return Ok(UdpPacket { data: buf, addr });
            }
            Err(e) if udp_recv_error_is_transient(&e) => {}
            Err(e) => return Err(format!("udp_recv_from() failed: {}", e)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_from_with_timeout(
    socket: &std::net::UdpSocket,
    max_bytes: usize,
    timeout_ms: i64,
) -> Result<Option<UdpPacket>, String> {
    socket.set_nonblocking(true).map_err(|e| {
        format!(
            "udp_recv_from_timeout() failed to enter nonblocking mode: {}",
            e
        )
    })?;
    let result = udp_recv_from_nonblocking_loop(socket, max_bytes, timeout_ms);
    if let Err(e) = socket.set_nonblocking(false) {
        return Err(format!(
            "udp_recv_from_timeout() failed to restore blocking mode: {}",
            e
        ));
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_from_nonblocking_loop(
    socket: &std::net::UdpSocket,
    max_bytes: usize,
    timeout_ms: i64,
) -> Result<Option<UdpPacket>, String> {
    let deadline = poll_deadline(timeout_ms, "udp_recv_from_timeout()")?;
    let mut buf = vec![0u8; max_bytes];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, addr)) => {
                let mut data = Vec::with_capacity(n);
                data.extend_from_slice(&buf[..n]);
                return Ok(Some(UdpPacket { data, addr }));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !sleep_until_next_poll(deadline) {
                    return Ok(None);
                }
            }
            Err(e) if udp_recv_error_is_transient(&e) => {
                if !sleep_until_next_poll(deadline) {
                    return Ok(None);
                }
            }
            Err(e) => return Err(format!("udp_recv_from_timeout() failed: {}", e)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_recv_error_is_transient(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionReset
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_deadline(timeout_ms: i64, builtin: &str) -> Result<Option<Instant>, String> {
    if timeout_ms == 0 {
        return Ok(None);
    }
    Instant::now()
        .checked_add(Duration::from_millis(timeout_ms as u64))
        .map(Some)
        .ok_or_else(|| format!("{} timeout_ms is too large", builtin))
}

#[cfg(not(target_arch = "wasm32"))]
fn sleep_until_next_poll(deadline: Option<Instant>) -> bool {
    let Some(deadline) = deadline else {
        return false;
    };
    let now = Instant::now();
    if now >= deadline {
        return false;
    }
    let remaining = deadline.saturating_duration_since(now);
    let sleep_for = if remaining > Duration::from_millis(1) {
        Duration::from_millis(1)
    } else {
        remaining
    };
    if !sleep_for.is_zero() {
        std::thread::sleep(sleep_for);
    }
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_packet_to_value(gc: &mut GcHeap, packet: UdpPacket) -> Value {
    let data = String::from_utf8_lossy(&packet.data).into_owned();
    let data_value = Value::from_string(gc, data);
    let host_value = Value::from_string(gc, packet.addr.ip().to_string());
    let port_value = Value::from_int(gc, i64::from(packet.addr.port()));
    Value::tuple(gc, vec![data_value, host_value, port_value])
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_packet_to_bytes_value(gc: &mut GcHeap, packet: UdpPacket) -> Value {
    let mut data = Vec::with_capacity(packet.data.len());
    for byte in packet.data {
        data.push(Value::from_int(gc, i64::from(byte)));
    }
    let data_value = Value::list(gc, data);
    let host_value = Value::from_string(gc, packet.addr.ip().to_string());
    let port_value = Value::from_int(gc, i64::from(packet.addr.port()));
    Value::tuple(gc, vec![data_value, host_value, port_value])
}

#[cfg(not(target_arch = "wasm32"))]
fn udp_packet_to_bytebuf_value(gc: &mut GcHeap, packet: UdpPacket) -> Value {
    let data_value = Value::bytebuf(gc, packet.data);
    let host_value = Value::from_string(gc, packet.addr.ip().to_string());
    let port_value = Value::from_int(gc, i64::from(packet.addr.port()));
    Value::tuple(gc, vec![data_value, host_value, port_value])
}

fn bytes_from_list_arg(value: &Value, fn_name: &str) -> Result<Vec<u8>, String> {
    let list = value.as_list().ok_or_else(|| {
        format!(
            "{} expects data list<int>, got {}",
            fn_name,
            value.type_name()
        )
    })?;
    let mut bytes = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        let n = item
            .as_int()
            .ok_or_else(|| format!("{} data element {} is not an int", fn_name, i))?;
        if !(0..=255).contains(&n) {
            return Err(format!(
                "{} byte value {} out of range 0..255 at index {}",
                fn_name, n, i
            ));
        }
        bytes.push(n as u8);
    }
    Ok(bytes)
}

fn bytes_from_bytebuf_arg<'a>(value: &'a Value, fn_name: &str) -> Result<&'a [u8], String> {
    value
        .as_bytebuf()
        .map(|bytes| bytes.as_slice())
        .ok_or_else(|| format!("{} expects bytebuf, got {}", fn_name, value.type_name()))
}

fn bytebuf_index_arg(value: &Value, what: &str) -> Result<usize, String> {
    let index = value
        .as_int()
        .ok_or_else(|| format!("{} expects int, got {}", what, value.type_name()))?;
    if index < 0 {
        return Err(format!("{} must be non-negative", what));
    }
    usize::try_from(index).map_err(|_| format!("{} is too large", what))
}

fn bytebuf_u8_arg(value: &Value, what: &str) -> Result<u8, String> {
    let byte = value
        .as_int()
        .ok_or_else(|| format!("{} expects int, got {}", what, value.type_name()))?;
    if !(0..=255).contains(&byte) {
        return Err(format!("{} {} out of range 0..255", what, byte));
    }
    Ok(byte as u8)
}

fn bytebuf_write_u32_le(
    bytes: &mut [u8],
    offset: usize,
    value: u32,
    fn_name: &str,
) -> Result<(), String> {
    if offset + 4 > bytes.len() {
        return Err(format!(
            "{} offset {} out of bounds for 4-byte write (len {})",
            fn_name,
            offset,
            bytes.len()
        ));
    }
    bytes[offset] = (value & 0xff) as u8;
    bytes[offset + 1] = ((value >> 8) & 0xff) as u8;
    bytes[offset + 2] = ((value >> 16) & 0xff) as u8;
    bytes[offset + 3] = ((value >> 24) & 0xff) as u8;
    Ok(())
}

fn bytebuf_read_u32_le(bytes: &[u8], offset: usize, fn_name: &str) -> Result<u32, String> {
    if offset + 4 > bytes.len() {
        return Err(format!(
            "{} offset {} out of bounds for 4-byte read (len {})",
            fn_name,
            offset,
            bytes.len()
        ));
    }
    Ok(u32::from(bytes[offset])
        | (u32::from(bytes[offset + 1]) << 8)
        | (u32::from(bytes[offset + 2]) << 16)
        | (u32::from(bytes[offset + 3]) << 24))
}

pub(crate) fn bi_regex_is_match(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("regex_is_match() requires exactly 2 arguments".into());
    }
    let pattern = args[0].as_str().ok_or_else(|| {
        format!(
            "regex_is_match() expects pattern string, got {}",
            args[0].type_name()
        )
    })?;
    let text = args[1].as_str().ok_or_else(|| {
        format!(
            "regex_is_match() expects text string, got {}",
            args[1].type_name()
        )
    })?;
    let regex = Regex::new(pattern)
        .map_err(|e| format!("regex_is_match() invalid pattern '{}': {}", pattern, e))?;
    Ok(Value::from_bool(regex.is_match(text)))
}

pub(crate) fn bi_regex_find(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("regex_find() requires exactly 2 arguments".into());
    }
    let pattern = args[0].as_str().ok_or_else(|| {
        format!(
            "regex_find() expects pattern string, got {}",
            args[0].type_name()
        )
    })?;
    let text = args[1].as_str().ok_or_else(|| {
        format!(
            "regex_find() expects text string, got {}",
            args[1].type_name()
        )
    })?;
    let regex = Regex::new(pattern)
        .map_err(|e| format!("regex_find() invalid pattern '{}': {}", pattern, e))?;
    let found = regex
        .find(text)
        .map(|m| Value::from_string(gc, m.as_str().to_string()));
    Ok(wrap_option(gc, found))
}

pub(crate) fn bi_now_unix_s(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if !args.is_empty() {
        return Err("now_unix_s() takes no arguments".into());
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("now_unix_s() failed: {}", e))?
        .as_secs();
    let out = i64::try_from(secs).map_err(|_| "now_unix_s() overflow".to_string())?;
    Ok(Value::from_int(gc, out))
}

pub(crate) fn bi_now_unix_ms(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if !args.is_empty() {
        return Err("now_unix_ms() takes no arguments".into());
    }
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("now_unix_ms() failed: {}", e))?
        .as_millis();
    let out = i64::try_from(ms).map_err(|_| "now_unix_ms() overflow".to_string())?;
    Ok(Value::from_int(gc, out))
}

pub(crate) fn bi_chr(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("chr() requires 1 argument".into());
    }
    let code = args[0]
        .as_int()
        .ok_or_else(|| format!("chr() expects int, got {}", args[0].type_name()))?;
    let ch = char::from_u32(code as u32)
        .ok_or_else(|| format!("chr(): invalid Unicode code point {}", code))?;
    Ok(Value::from_string(gc, ch.to_string()))
}

pub(crate) fn bi_ord(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("ord() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("ord() expects string, got {}", args[0].type_name()))?;
    let ch = s
        .chars()
        .next()
        .ok_or_else(|| "ord() called on empty string".to_string())?;
    Ok(Value::from_int(gc, ch as i64))
}

pub(crate) fn bi_chars(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("chars() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("chars() expects string, got {}", args[0].type_name()))?;
    let result: Vec<Value> = s
        .chars()
        .map(|c| Value::from_string(gc, c.to_string()))
        .collect();
    Ok(Value::list(gc, result))
}

pub(crate) fn bi_to_upper(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("to_upper() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("to_upper() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_string(gc, s.to_uppercase()))
}

pub(crate) fn bi_to_lower(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("to_lower() requires 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("to_lower() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_string(gc, s.to_lowercase()))
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

pub(crate) fn bi_byte_at(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("byte_at() requires exactly 2 arguments (string, index)".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("byte_at() expects string, got {}", args[0].type_name()))?;
    let idx = args[1]
        .as_int()
        .ok_or_else(|| format!("byte_at() expects int index, got {}", args[1].type_name()))?;

    if idx < 0 {
        return Err("byte_at() index cannot be negative".into());
    }
    let uidx = usize::try_from(idx).map_err(|_| format!("byte_at() index {} is too large", idx))?;
    let bytes = s.as_bytes();
    if uidx >= bytes.len() {
        return Err(format!(
            "byte_at() index {} out of bounds (len {})",
            uidx,
            bytes.len()
        ));
    }

    Ok(Value::from_int(gc, bytes[uidx] as i64))
}

pub(crate) fn bi_substring_bytes(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 3 {
        return Err("substring_bytes() requires exactly 3 arguments (string, start, end)".into());
    }
    let s = args[0].as_str().ok_or_else(|| {
        format!(
            "substring_bytes() expects string, got {}",
            args[0].type_name()
        )
    })?;
    let start = args[1].as_int().ok_or_else(|| {
        format!(
            "substring_bytes() expects int start, got {}",
            args[1].type_name()
        )
    })?;
    let end = args[2].as_int().ok_or_else(|| {
        format!(
            "substring_bytes() expects int end, got {}",
            args[2].type_name()
        )
    })?;

    if start < 0 {
        return Err("substring_bytes() start cannot be negative".into());
    }
    if end < 0 {
        return Err("substring_bytes() end cannot be negative".into());
    }
    if start > end {
        return Err("substring_bytes() start cannot be greater than end".into());
    }

    let ustart = usize::try_from(start)
        .map_err(|_| format!("substring_bytes() start {} is too large", start))?;
    let uend =
        usize::try_from(end).map_err(|_| format!("substring_bytes() end {} is too large", end))?;
    let bytes = s.as_bytes();

    if uend > bytes.len() {
        return Err(format!(
            "substring_bytes() end {} out of bounds (len {})",
            uend,
            bytes.len()
        ));
    }

    // We must ensure the byte slice is valid UTF-8, otherwise we'd create an invalid string
    let slice = &bytes[ustart..uend];
    match std::str::from_utf8(slice) {
        Ok(valid_str) => Ok(Value::from_string(gc, valid_str.to_string())),
        Err(_) => Err(format!(
            "substring_bytes() range {}..{} does not form valid UTF-8",
            ustart, uend
        )),
    }
}

pub(crate) fn bi_byte_len(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("byte_len() requires exactly 1 argument".into());
    }
    let s = args[0]
        .as_str()
        .ok_or_else(|| format!("byte_len() expects string, got {}", args[0].type_name()))?;
    Ok(Value::from_int(gc, s.len() as i64))
}

pub(crate) fn bi_gen_int(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(100);
    out.push(Value::from_int(gc, 0));
    for k in 1..=48 {
        out.push(Value::from_int(gc, k));
        out.push(Value::from_int(gc, -k));
    }
    out.push(Value::from_int(gc, 49));
    out.push(Value::from_int(gc, i64::MAX / 2));
    out.push(Value::from_int(gc, i64::MIN / 2));
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_float(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(100);
    out.push(Value::from_float(0.0));
    for k in 1..=48 {
        out.push(Value::from_float(k as f64));
        out.push(Value::from_float(-(k as f64)));
    }
    out.push(Value::from_float(f64::INFINITY));
    out.push(Value::from_float(f64::NEG_INFINITY));
    out.push(Value::from_float(f64::NAN));
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_str(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    let mut out = Vec::with_capacity(21);
    out.push(Value::from_string(gc, String::new()));
    for len in 1..=20 {
        out.push(Value::from_string(gc, "a".repeat(len)));
    }
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_gen_bool(gc: &mut GcHeap, _args: Vec<Value>) -> Result<Value, String> {
    Ok(Value::list(gc, vec![Value::TRUE, Value::FALSE]))
}

pub(crate) fn bi_gen_list(gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.is_empty() {
        return Err("gen_list() requires 1 argument".into());
    }
    let items = args[0]
        .as_list()
        .ok_or_else(|| format!("gen_list() expects list, got {}", args[0].type_name()))?;
    let n = items.len();
    let mut out = Vec::with_capacity(n + 1);
    for end in 0..=n {
        out.push(Value::list(gc, items.slice(0, end)));
    }
    Ok(Value::list(gc, out))
}

pub(crate) fn bi_assert(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("assert() requires 2 arguments".into());
    }
    if !args[0].is_truthy() {
        return Err(args[1].print_display());
    }
    Ok(Value::NIL)
}

pub(crate) fn bi_assert_eq(_gc: &mut GcHeap, args: Vec<Value>) -> Result<Value, String> {
    if args.len() < 2 {
        return Err("assert_eq() requires 2 arguments".into());
    }
    if args[0] != args[1] {
        return Err(format!(
            "assert_eq failed: {} != {}",
            args[0].print_display(),
            args[1].print_display()
        ));
    }
    Ok(Value::NIL)
}

impl VM {
    fn bi_flat_map(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("flat_map() requires 2 arguments".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            let gc = &mut self.gc;
            s.chars()
                .map(|c| Value::from_string(gc, c.to_string()))
                .collect()
        } else {
            return Err(format!(
                "flat_map() expects list or string, got {}",
                list.type_name()
            ));
        };

        let mut result = Vec::new();
        for item in items.into_iter() {
            let mapped = self.call_value(&func, vec![item])?;
            let sub_items = mapped.as_list().ok_or_else(|| {
                format!(
                    "flat_map() callback must return a list, got {}",
                    mapped.type_name()
                )
            })?;
            result.extend(sub_items.iter().cloned());
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_group_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("group_by() requires 2 arguments".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let func = arg_iter.next().unwrap();

        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            let gc = &mut self.gc;
            s.chars()
                .map(|c| Value::from_string(gc, c.to_string()))
                .collect()
        } else {
            return Err(format!(
                "group_by() expects list or string, got {}",
                list.type_name()
            ));
        };

        // real map keys (str, int, bool, entity, tuple) — invalid key
        // types (float, nil, …) error instead of silently stringifying
        let mut groups: HashMap<MapKey, Vec<Value>> = HashMap::new();
        for item in items.into_iter() {
            let key_value = self.call_value(&func, vec![item])?;
            let key = MapKey::from_value(&key_value)
                .map_err(|e| format!("group_by() key function: {}", e))?;
            groups.entry(key).or_default().push(item);
        }
        let gc = &mut self.gc;
        let out: MapStorage = groups
            .into_iter()
            .map(|(k, vs)| (k, Value::list(gc, vs)))
            .collect();
        Ok(Value::map(gc, out))
    }

    fn bi_sort_by(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err("sort_by() requires 2 arguments (list, key_fn)".into());
        }
        let mut arg_iter = args.into_iter();
        let list = arg_iter.next().unwrap();
        let got = list.type_name();
        let key_fn = arg_iter.next().unwrap();

        let is_string = list.as_str().is_some();
        let items = if list.as_list().is_some() {
            list.into_rad_list().unwrap().into_vec()
        } else if let Some(s) = list.as_str() {
            let gc = &mut self.gc;
            s.chars()
                .map(|c| Value::from_string(gc, c.to_string()))
                .collect()
        } else {
            return Err(format!("sort_by() expects list or string, got {}", got));
        };

        let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(items.len());
        for item in items.into_iter() {
            let key = self.call_value(&key_fn, vec![item])?;
            keyed.push((key, item));
        }

        // The shared value order: numbers, strings, bools, and tuple keys
        // (lexicographic) — multi-key sorting is `sort_by` with a tuple.
        let mut err: Option<String> = None;
        keyed.sort_by(|(a, _), (b, _)| match helpers::compare_values(a, b) {
            Ok(ord) => ord,
            Err(e) => {
                if err.is_none() {
                    err = Some(format!(
                        "sort_by() key function returned incomparable keys: {}",
                        e
                    ));
                }
                std::cmp::Ordering::Equal
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        let result: Vec<Value> = keyed.into_iter().map(|(_, v)| v).collect();

        let gc = &mut self.gc;
        if is_string {
            let s: String = result
                .into_iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            Ok(Value::from_string(gc, s))
        } else {
            Ok(Value::list(gc, result))
        }
    }

    fn bi_load_extension(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            return Err("load_extension() requires 1 argument (path)".into());
        }
        let path_val = &args[0];
        let path = path_val.as_str().ok_or_else(|| {
            format!(
                "load_extension() expects string, got {}",
                path_val.type_name()
            )
        })?;

        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("Plugins are not supported on wasm32".to_string());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let (functions, lib) = crate::ffi::load_plugin(path, &mut self.gc)?;

            self.loaded_libraries.push(lib);

            let mut map = MapStorage::new();
            for (name, info) in functions {
                map.insert(MapKey::Str(name), Value::from_native_fn(&mut self.gc, info));
            }

            Ok(Value::map(&mut self.gc, map))
        }
    }

    // â”€â”€ Tier 1: Standard I/O â”€â”€

    fn bi_eprint(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let s = args
            .iter()
            .map(|v| v.print_display())
            .collect::<Vec<_>>()
            .join(" ");
        self.eprint_buffer.push(s.clone());
        if !self.suppress_output {
            eprintln!("{}", s);
        }
        Ok(Value::NIL)
    }

    fn bi_write_stdout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("write_stdout() requires exactly 1 argument".into());
        }
        let s = args[0]
            .as_str()
            .ok_or_else(|| format!("write_stdout() expects string, got {}", args[0].type_name()))?;
        self.print_buffer.push(s.to_string());
        if !self.suppress_output {
            use std::io::Write;
            print!("{}", s);
            let _ = std::io::stdout().flush();
        }
        Ok(Value::NIL)
    }

    fn bi_write_stderr(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("write_stderr() requires exactly 1 argument".into());
        }
        let s = args[0]
            .as_str()
            .ok_or_else(|| format!("write_stderr() expects string, got {}", args[0].type_name()))?;
        self.eprint_buffer.push(s.to_string());
        if !self.suppress_output {
            use std::io::Write;
            eprint!("{}", s);
            let _ = std::io::stderr().flush();
        }
        Ok(Value::NIL)
    }

    fn bi_read_stdin_all(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("read_stdin_all() takes no arguments".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            return Err("read_stdin_all() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                return Ok(self.spawn_io_task(move || {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .map_err(|e| format!("read_stdin_all() failed: {}", e))?;
                    Ok(IoTaskPayload::String(buf))
                }));
            }
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("read_stdin_all() failed: {}", e))?;
            let gc = &mut self.gc;
            Ok(Value::from_string(gc, buf))
        }
    }

    fn bi_sleep_ms(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("sleep_ms() requires 1 argument".into());
        }
        let ms = args[0]
            .as_int()
            .or_else(|| args[0].as_float().map(|f| f as i64))
            .ok_or_else(|| format!("sleep_ms() expects int, got {}", args[0].type_name()))?;
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        }
        Ok(Value::NIL)
    }

    fn bi_flush_stdout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("flush_stdout() takes no arguments".into());
        }
        use std::io::Write;
        std::io::stdout()
            .flush()
            .map_err(|e| format!("flush_stdout() failed: {}", e))?;
        Ok(Value::NIL)
    }

    // â”€â”€ Tier 2: File I/O â”€â”€

    fn bi_append_file(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("append_file() requires exactly 2 arguments".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "append_file() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        let content = args[1].as_str().ok_or_else(|| {
            format!(
                "append_file() expects content string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, content);
            return Err("append_file() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                let content_owned = content.to_string();
                return Ok(self.spawn_io_task(move || {
                    use std::io::Write;
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path_owned)
                        .map_err(|e| format!("append_file() failed for '{}': {}", path_owned, e))?;
                    file.write_all(content_owned.as_bytes())
                        .map_err(|e| format!("append_file() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("append_file() failed for '{}': {}", path, e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("append_file() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_file_exists(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("file_exists() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "file_exists() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("file_exists() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let exists = std::path::Path::new(&path_owned).exists();
                    Ok(IoTaskPayload::Int(if exists { 1 } else { 0 }))
                }));
            }
            Ok(Value::from_bool(std::path::Path::new(path).exists()))
        }
    }

    fn bi_remove_file(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("remove_file() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "remove_file() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("remove_file() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::remove_file(&path_owned)
                        .map_err(|e| format!("remove_file() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::remove_file(path)
                .map_err(|e| format!("remove_file() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_list_dir(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("list_dir() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "list_dir() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("list_dir() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let mut names = Vec::new();
                    let entries = fs::read_dir(&path_owned)
                        .map_err(|e| format!("list_dir() failed for '{}': {}", path_owned, e))?;
                    for entry in entries {
                        let entry = entry.map_err(|e| format!("list_dir() entry error: {}", e))?;
                        names.push(entry.file_name().to_string_lossy().into_owned());
                    }
                    Ok(IoTaskPayload::StringList(names))
                }));
            }
            let mut result = Vec::new();
            let entries = fs::read_dir(path)
                .map_err(|e| format!("list_dir() failed for '{}': {}", path, e))?;
            let gc = &mut self.gc;
            for entry in entries {
                let entry = entry.map_err(|e| format!("list_dir() entry error: {}", e))?;
                result.push(Value::from_string(
                    gc,
                    entry.file_name().to_string_lossy().into_owned(),
                ));
            }
            Ok(Value::list(gc, result))
        }
    }

    fn bi_create_dir(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("create_dir() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "create_dir() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("create_dir() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::create_dir_all(&path_owned)
                        .map_err(|e| format!("create_dir() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::create_dir_all(path)
                .map_err(|e| format!("create_dir() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_remove_dir(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("remove_dir() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "remove_dir() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("remove_dir() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::remove_dir_all(&path_owned)
                        .map_err(|e| format!("remove_dir() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::remove_dir_all(path)
                .map_err(|e| format!("remove_dir() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_read_file_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("read_file_bytes() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "read_file_bytes() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("read_file_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let bytes = fs::read(&path_owned).map_err(|e| {
                        format!("read_file_bytes() failed for '{}': {}", path_owned, e)
                    })?;
                    Ok(IoTaskPayload::Bytes(bytes))
                }));
            }
            let bytes = fs::read(path)
                .map_err(|e| format!("read_file_bytes() failed for '{}': {}", path, e))?;
            let gc = &mut self.gc;
            let values: Vec<Value> = bytes
                .into_iter()
                .map(|b| Value::from_int(gc, b as i64))
                .collect();
            Ok(Value::list(gc, values))
        }
    }

    fn bi_write_file_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("write_file_bytes() requires exactly 2 arguments".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "write_file_bytes() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        let list = args[1].as_list().ok_or_else(|| {
            format!(
                "write_file_bytes() expects list of ints, got {}",
                args[1].type_name()
            )
        })?;
        let mut bytes = Vec::with_capacity(list.len());
        for (i, v) in list.iter().enumerate() {
            let n = v
                .as_int()
                .ok_or_else(|| format!("write_file_bytes() list element {} is not an int", i))?;
            if !(0..=255).contains(&n) {
                return Err(format!(
                    "write_file_bytes() byte value {} out of range 0..255 at index {}",
                    n, i
                ));
            }
            bytes.push(n as u8);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, bytes);
            return Err("write_file_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::write(&path_owned, &bytes).map_err(|e| {
                        format!("write_file_bytes() failed for '{}': {}", path_owned, e)
                    })?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::write(path, &bytes)
                .map_err(|e| format!("write_file_bytes() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_http_post(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("http_post() requires exactly 2 arguments".into());
        }
        let url = args[0].as_str().ok_or_else(|| {
            format!(
                "http_post() expects url string, got {}",
                args[0].type_name()
            )
        })?;
        let body = args[1].as_str().ok_or_else(|| {
            format!(
                "http_post() expects body string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (url, body);
            return Err("http_post() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let url_owned = url.to_string();
                let body_owned = body.to_string();
                return Ok(self.spawn_io_task(move || {
                    let response =
                        ureq::post(&url_owned)
                            .send(body_owned.as_bytes())
                            .map_err(|e| {
                                format!("http_post() request failed for '{}': {}", url_owned, e)
                            })?;
                    let text = response
                        .into_body()
                        .read_to_string()
                        .map_err(|e| format!("http_post() failed reading response body: {}", e))?;
                    Ok(IoTaskPayload::String(text))
                }));
            }
            let response = ureq::post(url)
                .send(body.as_bytes())
                .map_err(|e| format!("http_post() request failed for '{}': {}", url, e))?;
            let text = response
                .into_body()
                .read_to_string()
                .map_err(|e| format!("http_post() failed reading response body: {}", e))?;
            let gc = &mut self.gc;
            Ok(Value::from_string(gc, text))
        }
    }

    fn bi_http_post_json(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("http_post_json() requires exactly 2 arguments".into());
        }
        let url = args[0].as_str().ok_or_else(|| {
            format!(
                "http_post_json() expects url string, got {}",
                args[0].type_name()
            )
        })?;
        let body = args[1].as_str().ok_or_else(|| {
            format!(
                "http_post_json() expects body string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (url, body);
            return Err("http_post_json() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let url_owned = url.to_string();
                let body_owned = body.to_string();
                return Ok(self.spawn_io_task(move || {
                    let response = ureq::post(&url_owned)
                        .content_type("application/json")
                        .send(body_owned.as_bytes())
                        .map_err(|e| {
                            format!("http_post_json() request failed for '{}': {}", url_owned, e)
                        })?;
                    let mut resp_body = response.into_body();
                    let text = resp_body.read_to_string().map_err(|e| {
                        format!("http_post_json() failed reading response body: {}", e)
                    })?;
                    Ok(IoTaskPayload::String(text))
                }));
            }
            let response = ureq::post(url)
                .content_type("application/json")
                .send(body.as_bytes())
                .map_err(|e| format!("http_post_json() request failed for '{}': {}", url, e))?;
            let mut resp_body = response.into_body();
            let text = resp_body
                .read_to_string()
                .map_err(|e| format!("http_post_json() failed reading response body: {}", e))?;
            Ok(Value::from_string(&mut self.gc, text))
        }
    }

    fn bi_http_request(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "http_request() requires exactly 4 arguments: method, url, headers, body".into(),
            );
        }
        let method = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "http_request() expects method string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let url = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "http_request() expects url string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let headers_map = args[2].as_map().ok_or_else(|| {
            format!(
                "http_request() expects headers map, got {}",
                args[2].type_name()
            )
        })?;
        let mut headers: Vec<(String, String)> = Vec::new();
        for (k, v) in headers_map.iter() {
            let key = match k {
                MapKey::Str(s) => s.clone(),
                other => {
                    return Err(format!(
                        "http_request() header key must be string, got {}",
                        other
                    ))
                }
            };
            let val = v
                .as_str()
                .ok_or_else(|| {
                    format!(
                        "http_request() header value must be string, got {}",
                        v.type_name()
                    )
                })?
                .to_string();
            headers.push((key, val));
        }
        let body = args[3]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "http_request() expects body string, got {}",
                    args[3].type_name()
                )
            })?
            .to_string();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (method, url, headers, body);
            return Err("http_request() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            type HttpResponse = Result<(u16, String, Vec<(String, String)>), String>;
            let do_request = move || -> HttpResponse {
                let method_upper = method.to_uppercase();
                let send_with_headers =
                    |mut req: ureq::RequestBuilder<ureq::typestate::WithBody>,
                     hdrs: &[(String, String)],
                     b: &str|
                     -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
                        for (k, v) in hdrs {
                            req = req.header(k.as_str(), v.as_str());
                        }
                        if b.is_empty() {
                            req.send_empty()
                        } else {
                            req.send(b.as_bytes())
                        }
                    };
                let call_no_body =
                    |mut req: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
                     hdrs: &[(String, String)]|
                     -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
                        for (k, v) in hdrs {
                            req = req.header(k.as_str(), v.as_str());
                        }
                        req.call()
                    };
                let response = match method_upper.as_str() {
                    "GET" => call_no_body(ureq::get(&url), &headers),
                    "HEAD" => call_no_body(ureq::head(&url), &headers),
                    "DELETE" => call_no_body(ureq::delete(&url), &headers).or_else(|_| {
                        send_with_headers(ureq::delete(&url).force_send_body(), &headers, &body)
                    }),
                    "POST" => send_with_headers(ureq::post(&url), &headers, &body),
                    "PUT" => send_with_headers(ureq::put(&url), &headers, &body),
                    "PATCH" => send_with_headers(ureq::patch(&url), &headers, &body),
                    other => return Err(format!("http_request() unsupported method: {}", other)),
                };
                let response = response.map_err(|e| format!("http_request() failed: {}", e))?;
                let status = response.status().as_u16();
                let mut resp_headers: Vec<(String, String)> = Vec::new();
                for (name, val) in response.headers().iter() {
                    let n: &ureq::http::HeaderName = name;
                    let v: &ureq::http::HeaderValue = val;
                    resp_headers.push((n.to_string(), v.to_str().unwrap_or("").to_string()));
                }
                let text = response
                    .into_body()
                    .read_to_string()
                    .map_err(|e| format!("http_request() failed reading body: {}", e))?;
                Ok((status, text, resp_headers))
            };
            if self.in_async_context {
                return Ok(self.spawn_io_task(move || {
                    let (status, text, resp_headers) = do_request()?;
                    let mut header_pairs: Vec<(String, IoTaskPayload)> = Vec::new();
                    for (k, v) in resp_headers {
                        header_pairs.push((k, IoTaskPayload::String(v)));
                    }
                    Ok(IoTaskPayload::ValueMap(vec![
                        ("status".to_string(), IoTaskPayload::Int(status as i64)),
                        ("body".to_string(), IoTaskPayload::String(text)),
                        ("headers".to_string(), IoTaskPayload::ValueMap(header_pairs)),
                    ]))
                }));
            }
            let (status, text, resp_headers) = do_request()?;
            let mut result_map = MapStorage::new();
            result_map.insert(
                MapKey::Str("status".to_string()),
                Value::from_int(&mut self.gc, status as i64),
            );
            result_map.insert(
                MapKey::Str("body".to_string()),
                Value::from_string(&mut self.gc, text),
            );
            let mut hdr_map = MapStorage::new();
            for (k, v) in resp_headers {
                hdr_map.insert(MapKey::Str(k), Value::from_string(&mut self.gc, v));
            }
            result_map.insert(
                MapKey::Str("headers".to_string()),
                Value::map(&mut self.gc, hdr_map),
            );
            Ok(Value::map(&mut self.gc, result_map))
        }
    }

    // â”€â”€ Tier 4: TCP Networking â”€â”€

    fn bi_tcp_connect(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_connect() requires exactly 2 arguments: host, port".into());
        }
        let host = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "tcp_connect() expects host string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let port = args[1].as_int().ok_or_else(|| {
            format!(
                "tcp_connect() expects port int, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (host, port);
            return Err("tcp_connect() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let addr = format!("{}:{}", host, port);
            let stream = std::net::TcpStream::connect(&addr)
                .map_err(|e| format!("tcp_connect() failed for '{}': {}", addr, e))?;
            let handle_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(handle_id, super::NetHandle::TcpStream(stream));
            let gc = &mut self.gc;
            Ok(Value::from_int(gc, handle_id as i64))
        }
    }

    fn bi_tcp_listen(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_listen() requires exactly 2 arguments: host, port".into());
        }
        let host = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "tcp_listen() expects host string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let port = args[1]
            .as_int()
            .ok_or_else(|| format!("tcp_listen() expects port int, got {}", args[1].type_name()))?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (host, port);
            return Err("tcp_listen() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let addr = format!("{}:{}", host, port);
            let listener = std::net::TcpListener::bind(&addr)
                .map_err(|e| format!("tcp_listen() failed for '{}': {}", addr, e))?;
            let handle_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(handle_id, super::NetHandle::TcpListener(listener));
            let gc = &mut self.gc;
            Ok(Value::from_int(gc, handle_id as i64))
        }
    }

    fn bi_tcp_accept(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("tcp_accept() requires exactly 1 argument: listener handle".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_accept() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handle_id;
            return Err("tcp_accept() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let listener = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::TcpListener(l)) => l,
                Some(_) => return Err("tcp_accept() handle is not a TcpListener".into()),
                None => return Err(format!("tcp_accept() invalid handle {}", handle_id)),
            };
            let (stream, _addr) = listener
                .accept()
                .map_err(|e| format!("tcp_accept() failed: {}", e))?;
            let client_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(client_id, super::NetHandle::TcpStream(stream));
            let gc = &mut self.gc;
            Ok(Value::from_int(gc, client_id as i64))
        }
    }

    fn bi_tcp_accept_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(
                "tcp_accept_timeout() requires exactly 2 arguments: listener handle, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_accept_timeout() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let timeout_ms = args[1].as_int().ok_or_else(|| {
            format!(
                "tcp_accept_timeout() expects timeout_ms int, got {}",
                args[1].type_name()
            )
        })?;
        if timeout_ms < 0 {
            return Err("tcp_accept_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, timeout_ms);
            return Err("tcp_accept_timeout() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let maybe_stream = {
                let listener = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::TcpListener(l)) => l,
                    Some(_) => {
                        return Err("tcp_accept_timeout() handle is not a TcpListener".into())
                    }
                    None => {
                        return Err(format!("tcp_accept_timeout() invalid handle {}", handle_id))
                    }
                };
                tcp_accept_with_timeout(listener, timeout_ms)?
            };

            match maybe_stream {
                Some(stream) => {
                    let client_id = self.next_net_handle_id;
                    self.next_net_handle_id += 1;
                    self.net_handles
                        .insert(client_id, super::NetHandle::TcpStream(stream));
                    let value = Value::from_int(&mut self.gc, client_id as i64);
                    Ok(wrap_option(&mut self.gc, Some(value)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_tcp_read(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_read() requires exactly 2 arguments: handle, max_bytes".into());
        }
        let handle_id = args[0]
            .as_int()
            .ok_or_else(|| format!("tcp_read() expects handle int, got {}", args[0].type_name()))?
            as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "tcp_read() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("tcp_read() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("tcp_read() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Read;
            let stream = match self.net_handles.get_mut(&handle_id) {
                Some(super::NetHandle::TcpStream(s)) => s,
                Some(_) => return Err("tcp_read() handle is not a TcpStream".into()),
                None => return Err(format!("tcp_read() invalid handle {}", handle_id)),
            };
            let mut buf = vec![0u8; max_bytes as usize];
            let n = stream
                .read(&mut buf)
                .map_err(|e| format!("tcp_read() failed: {}", e))?;
            buf.truncate(n);
            let text = String::from_utf8_lossy(&buf).into_owned();
            Ok(Value::from_string(&mut self.gc, text))
        }
    }

    fn bi_tcp_write(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_write() requires exactly 2 arguments: handle, data".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_write() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let data = args[1].as_str().ok_or_else(|| {
            format!(
                "tcp_write() expects data string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, data);
            return Err("tcp_write() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Write;
            let stream = match self.net_handles.get_mut(&handle_id) {
                Some(super::NetHandle::TcpStream(s)) => s,
                Some(_) => return Err("tcp_write() handle is not a TcpStream".into()),
                None => return Err(format!("tcp_write() invalid handle {}", handle_id)),
            };
            stream
                .write_all(data.as_bytes())
                .map_err(|e| format!("tcp_write() failed: {}", e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_tcp_close(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("tcp_close() requires exactly 1 argument: handle".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "tcp_close() expects handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handle_id;
            return Err("tcp_close() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.net_handles.remove(&handle_id).is_none() {
                return Err(format!("tcp_close() invalid handle {}", handle_id));
            }
            Ok(Value::NIL)
        }
    }

    fn bi_udp_bind(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("udp_bind() requires exactly 2 arguments: host, port".into());
        }
        let host = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_bind() expects host string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let port = args[1]
            .as_int()
            .ok_or_else(|| format!("udp_bind() expects port int, got {}", args[1].type_name()))?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (host, port);
            return Err("udp_bind() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let addr = format!("{}:{}", host, port);
            let socket = std::net::UdpSocket::bind(&addr)
                .map_err(|e| format!("udp_bind() failed for '{}': {}", addr, e))?;
            let handle_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(handle_id, super::NetHandle::UdpSocket(socket));
            Ok(Value::from_int(&mut self.gc, handle_id as i64))
        }
    }

    fn bi_udp_recv_from(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("udp_recv_from() requires exactly 2 arguments: socket, max_bytes".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("udp_recv_from() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_recv_from() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_recv_from() invalid handle {}", handle_id)),
            };
            let packet = udp_recv_from_blocking(socket, max_bytes as usize)?;
            Ok(udp_packet_to_value(&mut self.gc, packet))
        }
    }

    fn bi_udp_recv_from_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(
                "udp_recv_from_timeout() requires exactly 3 arguments: socket, max_bytes, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_timeout() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_timeout() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        let timeout_ms = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_timeout() expects timeout_ms int, got {}",
                args[2].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from_timeout() max_bytes must be positive".into());
        }
        if timeout_ms < 0 {
            return Err("udp_recv_from_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes, timeout_ms);
            return Err("udp_recv_from_timeout() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let packet = {
                let socket = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::UdpSocket(s)) => s,
                    Some(_) => {
                        return Err("udp_recv_from_timeout() handle is not a UdpSocket".into())
                    }
                    None => {
                        return Err(format!(
                            "udp_recv_from_timeout() invalid handle {}",
                            handle_id
                        ))
                    }
                };
                udp_recv_from_with_timeout(socket, max_bytes as usize, timeout_ms)?
            };
            match packet {
                Some(packet) => {
                    let tuple = udp_packet_to_value(&mut self.gc, packet);
                    Ok(wrap_option(&mut self.gc, Some(tuple)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_udp_recv_from_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(
                "udp_recv_from_bytes() requires exactly 2 arguments: socket, max_bytes".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from_bytes() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("udp_recv_from_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_recv_from_bytes() handle is not a UdpSocket".into()),
                None => {
                    return Err(format!(
                        "udp_recv_from_bytes() invalid handle {}",
                        handle_id
                    ))
                }
            };
            let packet = udp_recv_from_blocking(socket, max_bytes as usize)?;
            Ok(udp_packet_to_bytes_value(&mut self.gc, packet))
        }
    }

    fn bi_udp_recv_from_bytes_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(
                "udp_recv_from_bytes_timeout() requires exactly 3 arguments: socket, max_bytes, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes_timeout() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes_timeout() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        let timeout_ms = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_recv_from_bytes_timeout() expects timeout_ms int, got {}",
                args[2].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_from_bytes_timeout() max_bytes must be positive".into());
        }
        if timeout_ms < 0 {
            return Err("udp_recv_from_bytes_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes, timeout_ms);
            return Err(
                "udp_recv_from_bytes_timeout() is not supported in wasm runtime".to_string(),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let packet = {
                let socket = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::UdpSocket(s)) => s,
                    Some(_) => {
                        return Err("udp_recv_from_bytes_timeout() handle is not a UdpSocket".into())
                    }
                    None => {
                        return Err(format!(
                            "udp_recv_from_bytes_timeout() invalid handle {}",
                            handle_id
                        ))
                    }
                };
                udp_recv_from_with_timeout(socket, max_bytes as usize, timeout_ms)?
            };
            match packet {
                Some(packet) => {
                    let tuple = udp_packet_to_bytes_value(&mut self.gc, packet);
                    Ok(wrap_option(&mut self.gc, Some(tuple)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_udp_recv_bytebuf(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(
                "udp_recv_bytebuf() requires exactly 2 arguments: socket, max_bytes".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_bytebuf() max_bytes must be positive".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes);
            return Err("udp_recv_bytebuf() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_recv_bytebuf() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_recv_bytebuf() invalid handle {}", handle_id)),
            };
            let packet = udp_recv_from_blocking(socket, max_bytes as usize)?;
            Ok(udp_packet_to_bytebuf_value(&mut self.gc, packet))
        }
    }

    fn bi_udp_recv_bytebuf_timeout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(
                "udp_recv_bytebuf_timeout() requires exactly 3 arguments: socket, max_bytes, timeout_ms"
                    .into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf_timeout() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let max_bytes = args[1].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf_timeout() expects max_bytes int, got {}",
                args[1].type_name()
            )
        })?;
        let timeout_ms = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_recv_bytebuf_timeout() expects timeout_ms int, got {}",
                args[2].type_name()
            )
        })?;
        if max_bytes <= 0 {
            return Err("udp_recv_bytebuf_timeout() max_bytes must be positive".into());
        }
        if timeout_ms < 0 {
            return Err("udp_recv_bytebuf_timeout() timeout_ms must be non-negative".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, max_bytes, timeout_ms);
            return Err("udp_recv_bytebuf_timeout() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let packet = {
                let socket = match self.net_handles.get(&handle_id) {
                    Some(super::NetHandle::UdpSocket(s)) => s,
                    Some(_) => {
                        return Err("udp_recv_bytebuf_timeout() handle is not a UdpSocket".into())
                    }
                    None => {
                        return Err(format!(
                            "udp_recv_bytebuf_timeout() invalid handle {}",
                            handle_id
                        ))
                    }
                };
                udp_recv_from_with_timeout(socket, max_bytes as usize, timeout_ms)?
            };
            match packet {
                Some(packet) => {
                    let tuple = udp_packet_to_bytebuf_value(&mut self.gc, packet);
                    Ok(wrap_option(&mut self.gc, Some(tuple)))
                }
                None => Ok(wrap_option(&mut self.gc, None)),
            }
        }
    }

    fn bi_udp_send_to(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "udp_send_to() requires exactly 4 arguments: socket, host, port, data".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_send_to() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let host = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_to() expects host string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let port = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_send_to() expects port int, got {}",
                args[2].type_name()
            )
        })?;
        let data = args[3]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_to() expects data string, got {}",
                    args[3].type_name()
                )
            })?
            .to_string();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, host, port, data);
            return Err("udp_send_to() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_send_to() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_send_to() invalid handle {}", handle_id)),
            };
            let addr = format!("{}:{}", host, port);
            let sent = socket
                .send_to(data.as_bytes(), &addr)
                .map_err(|e| format!("udp_send_to() failed for '{}': {}", addr, e))?;
            Ok(Value::from_int(&mut self.gc, sent as i64))
        }
    }

    fn bi_udp_send_to_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "udp_send_to_bytes() requires exactly 4 arguments: socket, host, port, data".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_send_to_bytes() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let host = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_to_bytes() expects host string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let port = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_send_to_bytes() expects port int, got {}",
                args[2].type_name()
            )
        })?;
        let bytes = bytes_from_list_arg(&args[3], "udp_send_to_bytes()")?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, host, port, bytes);
            return Err("udp_send_to_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_send_to_bytes() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_send_to_bytes() invalid handle {}", handle_id)),
            };
            let addr = format!("{}:{}", host, port);
            let sent = socket
                .send_to(&bytes, &addr)
                .map_err(|e| format!("udp_send_to_bytes() failed for '{}': {}", addr, e))?;
            Ok(Value::from_int(&mut self.gc, sent as i64))
        }
    }

    fn bi_udp_send_bytebuf(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "udp_send_bytebuf() requires exactly 4 arguments: socket, host, port, data".into(),
            );
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_send_bytebuf() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        let host = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "udp_send_bytebuf() expects host string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let port = args[2].as_int().ok_or_else(|| {
            format!(
                "udp_send_bytebuf() expects port int, got {}",
                args[2].type_name()
            )
        })?;
        let bytes = bytes_from_bytebuf_arg(&args[3], "udp_send_bytebuf()")?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (handle_id, host, port, bytes);
            return Err("udp_send_bytebuf() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(s)) => s,
                Some(_) => return Err("udp_send_bytebuf() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_send_bytebuf() invalid handle {}", handle_id)),
            };
            let addr = format!("{}:{}", host, port);
            let sent = socket
                .send_to(bytes, &addr)
                .map_err(|e| format!("udp_send_bytebuf() failed for '{}': {}", addr, e))?;
            Ok(Value::from_int(&mut self.gc, sent as i64))
        }
    }

    fn bi_udp_close(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("udp_close() requires exactly 1 argument: socket handle".into());
        }
        let handle_id = args[0].as_int().ok_or_else(|| {
            format!(
                "udp_close() expects socket handle int, got {}",
                args[0].type_name()
            )
        })? as u64;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handle_id;
            return Err("udp_close() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.net_handles.get(&handle_id) {
                Some(super::NetHandle::UdpSocket(_)) => {}
                Some(_) => return Err("udp_close() handle is not a UdpSocket".into()),
                None => return Err(format!("udp_close() invalid handle {}", handle_id)),
            }
            self.net_handles.remove(&handle_id);
            Ok(Value::NIL)
        }
    }

    // â”€â”€ Tier 5: Runtime Queries â”€â”€

    fn bi_query_where(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err(
                "query_where() requires at least 2 arguments: component type(s) and predicate"
                    .into(),
            );
        }
        let pred = *args.last().unwrap();
        let comp_names: Vec<String> = args[..args.len() - 1]
            .iter()
            .map(|a| {
                a.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "query_where() component arg must be string, got {}",
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        for ctype in &comp_names {
            self.sandbox_check_read(ctype)?;
        }
        let entities = self.world.query(&comp_names, &[]);
        let mut result = Vec::new();
        for eid in entities {
            let eid_val = Value::from_entity_id(&mut self.gc, eid);
            let keep = self.call_value(&pred, vec![eid_val])?;
            if keep.is_truthy() {
                result.push(eid_val);
            }
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_query_map(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() < 2 {
            return Err(
                "query_map() requires at least 2 arguments: component type(s) and map function"
                    .into(),
            );
        }
        let map_fn = *args.last().unwrap();
        let comp_names: Vec<String> = args[..args.len() - 1]
            .iter()
            .map(|a| {
                a.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "query_map() component arg must be string, got {}",
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        for ctype in &comp_names {
            self.sandbox_check_read(ctype)?;
        }
        let entities = self.world.query(&comp_names, &[]);
        let mut result = Vec::with_capacity(entities.len());
        for eid in entities {
            let eid_val = Value::from_entity_id(&mut self.gc, eid);
            let mapped = self.call_value(&map_fn, vec![eid_val])?;
            result.push(mapped);
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_query_count(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.is_empty() {
            return Err("query_count() requires at least 1 component type argument".into());
        }
        let comp_names: Vec<String> = args
            .iter()
            .map(|a| {
                a.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    format!(
                        "query_count() component arg must be string, got {}",
                        a.type_name()
                    )
                })
            })
            .collect::<Result<_, _>>()?;

        for ctype in &comp_names {
            self.sandbox_check_read(ctype)?;
        }
        let entities = self.world.query(&comp_names, &[]);
        Ok(Value::from_int(&mut self.gc, entities.len() as i64))
    }

    fn bi_with_field(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err("with_field() requires 4 arguments: entity_list, component_type, field_name, predicate".into());
        }
        let pred = args.pop().unwrap();
        let field_name = args
            .pop()
            .unwrap()
            .as_str()
            .ok_or_else(|| "with_field() expects field name string".to_string())?
            .to_string();
        let comp_type = args
            .pop()
            .unwrap()
            .as_str()
            .ok_or_else(|| "with_field() expects component type string".to_string())?
            .to_string();
        self.sandbox_check_read(&comp_type)?;
        let entity_list = args.pop().unwrap();
        let type_name = entity_list.type_name().to_string();
        let entities = entity_list
            .into_rad_list()
            .ok_or_else(|| format!("with_field() expects entity list, got {}", type_name))?
            .into_vec();

        let mut result = Vec::new();
        for entity_val in entities.into_iter() {
            let eid = entity_val.as_entity_id().ok_or_else(|| {
                format!(
                    "with_field() list must contain entities, got {}",
                    entity_val.type_name()
                )
            })?;
            if let Some(comp) = self.world.get_component(eid, &comp_type) {
                if let Some(idx) = comp.layout.iter().position(|n| n == &field_name) {
                    if let Some(field_val) = comp.values.get(idx) {
                        let keep = self.call_value(&pred, vec![*field_val])?;
                        if keep.is_truthy() {
                            result.push(entity_val);
                        }
                    }
                }
            }
        }
        Ok(Value::list(&mut self.gc, result))
    }

    fn bi_variant_of(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!("variant_of expects 1 argument, got {}", args.len()));
        }
        let arg = args.pop().unwrap();
        let gc = &mut self.gc;
        if let Some(st) = arg.as_sum_type() {
            Ok(Value::from_string(gc, st.variant.clone()))
        } else if let Some(s) = arg.as_state() {
            Ok(Value::from_string(gc, s.state.clone()))
        } else {
            Ok(Value::NIL)
        }
    }

    fn bi_sys_args(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!("sys_args expects 0 arguments, got {}", args.len()));
        }
        // Program args only (what followed `--` on the CLI). Leaking the raw
        // process argv would expose the interpreter path and rad's own flags.
        let args: Vec<String> = self.sys_args.clone();
        let gc = &mut self.gc;
        let mut list = Vec::new();
        for arg in args {
            list.push(Value::from_string(gc, arg));
        }
        Ok(Value::list(gc, list))
    }
}

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
