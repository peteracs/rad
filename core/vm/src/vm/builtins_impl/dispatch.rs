// The builtin dispatch table, and the record/replay interposition every
// determinism-crossing builtin passes through on its way there.

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
        self.enforce_region_builtin(builtin)?;
        crate::builtins::validate_builtin_arity(builtin, args.len())?;
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
                let digest = crate::replay::args_digest(&args)?;
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
            let digest = crate::replay::args_digest(&args)?;
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
            Builtin::ReplaceFactBy => self.bi_resolver_fact_write(args, Builtin::ReplaceFactBy),
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
            Builtin::Range => self.bi_range(args),
            Builtin::Get => self.bi_get(args),
            Builtin::ReadField => self.bi_read_field(args),
            Builtin::Lookup => self.bi_lookup(args),
            Builtin::LookupAll => self.bi_lookup_all(args),
            Builtin::Require => self.bi_require(args),
            Builtin::RequireAll => self.bi_require_all(args),
            Builtin::Set => self.bi_set(args),
            Builtin::WriteField => self.bi_write_field(args),
            Builtin::Has => self.bi_has(args),
            Builtin::Spawn => self.bi_spawn(args),
            Builtin::GetEntity => self.bi_get_entity(args),
            Builtin::RequireEntity => self.bi_require_entity(args),
            Builtin::Remove => self.bi_remove(args),
            Builtin::RemoveMany => self.bi_remove_many(args),
            Builtin::Despawn => self.bi_despawn(args),
            Builtin::Entities => self.bi_entities(args),
            Builtin::VisitView => self.bi_visit_view(args),
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
            Builtin::HostTry => self.bi_host_try(args),
            Builtin::HostTry0 => self.bi_host_try0(args),
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
            Builtin::PublishBytes => self.bi_publish_bytes(args),
            Builtin::SizeOf => self.bi_size_of(args),
            Builtin::OffsetOf => self.bi_offset_of(args),
            Builtin::DecodeLe => self.bi_decode_native(args, true),
            Builtin::DecodeBe => self.bi_decode_native(args, false),
            Builtin::EncodeLe => self.bi_encode_native(args, true),
            Builtin::EncodeBe => self.bi_encode_native(args, false),
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
            Builtin::WhyFact => self.bi_why_fact(args),
            Builtin::ViewRevision => self.bi_view_revision(args),
            Builtin::ChangesSince => self.bi_changes_since(args),
            Builtin::WhyInView => self.bi_why_view(args, true),
            Builtin::WhyNotInView => self.bi_why_view(args, false),
            Builtin::WhyField => self.bi_why_field(args),
            Builtin::WhyRemoved => self.bi_why_removed(args),
            Builtin::WhyMissing => self.bi_why_missing(args),
            Builtin::WhyRevisionChanged => self.bi_why_revision_changed(args),
            Builtin::WhyRevisionDidNotChange => self.bi_why_revision_did_not_change(args),
            Builtin::LowerBound => self.bi_ordered_bound(args, false),
            Builtin::UpperBound | Builtin::Next => self.bi_ordered_bound(args, true),
            Builtin::Previous => self.bi_ordered_previous(args),
            Builtin::First => self.bi_ordered_edge(args, false),
            Builtin::Last => self.bi_ordered_edge(args, true),
            Builtin::EnterPhase => self.bi_enter_phase(args),
            Builtin::MarkPhase => self.bi_mark_phase(args),
            Builtin::AssertTrace => self.bi_assert_trace(args),
            Builtin::ModelCheck => self.bi_model_check(args),
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
}
