

impl Builtin {
    pub fn from_name(name: &str) -> Option<Builtin> {
        match name {
            "print" => Some(Builtin::Print),
            "len" => Some(Builtin::Len),
            "typeof" => Some(Builtin::TypeOf),
            "variant_of" => Some(Builtin::VariantOf),
            "sys_args" => Some(Builtin::SysArgs),
            "str" => Some(Builtin::Str),
            "int" => Some(Builtin::Int),
            "int_div" => Some(Builtin::IntDiv),
            "float" => Some(Builtin::Float),
            "abs" => Some(Builtin::Abs),
            "sign" => Some(Builtin::Sign),
            "popcount" => Some(Builtin::Popcount),
            "ctz" => Some(Builtin::Ctz),
            "shl" => Some(Builtin::Shl),
            "shr" => Some(Builtin::Shr),
            "filled" => Some(Builtin::Filled),
            "set_at" => Some(Builtin::SetAt),
            "sum" => Some(Builtin::Sum),
            "product" => Some(Builtin::Product),
            "get_or" => Some(Builtin::GetOr),
            "clamp" => Some(Builtin::Clamp),
            "index_of" => Some(Builtin::IndexOf),
            "any" => Some(Builtin::Any),
            "all" => Some(Builtin::All),
            "min" => Some(Builtin::Min),
            "max" => Some(Builtin::Max),
            "unwrap" => Some(Builtin::Unwrap),
            "expect" => Some(Builtin::Expect),
            "push" => Some(Builtin::Push),
            "pop" => Some(Builtin::Pop),
            "pop_last" => Some(Builtin::PopLast),
            "drop_last" => Some(Builtin::DropLast),
            "drop_first" => Some(Builtin::DropFirst),
            "recent_events" => Some(Builtin::RecentEvents),
            "sort" => Some(Builtin::Sort),
            "reverse" => Some(Builtin::Reverse),
            "slice" => Some(Builtin::Slice),
            "map" => Some(Builtin::Map),
            "filter" => Some(Builtin::Filter),
            "reduce" => Some(Builtin::Reduce),
            "range" => Some(Builtin::Range),
            "get" => Some(Builtin::Get),
            "lookup" => Some(Builtin::Lookup),
            "lookup_all" => Some(Builtin::LookupAll),
            "require" => Some(Builtin::Require),
            "require_all" => Some(Builtin::RequireAll),
            "set" => Some(Builtin::Set),
            "has" => Some(Builtin::Has),
            "spawn" => Some(Builtin::Spawn),
            "get_entity" => Some(Builtin::GetEntity),
            "require_entity" => Some(Builtin::RequireEntity),
            "name_of" => Some(Builtin::NameOf),
            "id_of" => Some(Builtin::IdOf),
            "remove" => Some(Builtin::Remove),
            "despawn" => Some(Builtin::Despawn),
            "entities" => Some(Builtin::Entities),
            "get_resource" => Some(Builtin::GetResource),
            "res" => Some(Builtin::Res),
            "set_resource" => Some(Builtin::SetResource),
            "transition" => Some(Builtin::Transition),
            "keys" => Some(Builtin::Keys),
            "contains" => Some(Builtin::Contains),
            "format" => Some(Builtin::Format),
            "entries" => Some(Builtin::Entries),
            "merge" => Some(Builtin::Merge),
            "remove_key" => Some(Builtin::RemoveKey),
            "group_by" => Some(Builtin::GroupBy),
            "split" => Some(Builtin::Split),
            "join" => Some(Builtin::Join),
            "trim" => Some(Builtin::Trim),
            "replace" => Some(Builtin::Replace),
            "starts_with" => Some(Builtin::StartsWith),
            "ends_with" => Some(Builtin::EndsWith),
            "append" => Some(Builtin::Append),
            "extend" => Some(Builtin::Extend),
            "zip" => Some(Builtin::Zip),
            "flat_map" => Some(Builtin::FlatMap),
            "enumerate" => Some(Builtin::Enumerate),
            "find" => Some(Builtin::Find),
            "max_by" => Some(Builtin::MaxBy),
            "min_by" => Some(Builtin::MinBy),
            "try_int" => Some(Builtin::TryInt),
            "try_float" => Some(Builtin::TryFloat),
            "chr" => Some(Builtin::Chr),
            "ord" => Some(Builtin::Ord),
            "chars" => Some(Builtin::Chars),
            "to_upper" => Some(Builtin::ToUpper),
            "to_lower" => Some(Builtin::ToLower),
            "values" => Some(Builtin::Values),
            "trace_id" => Some(Builtin::TraceId),
            "flush_events" => Some(Builtin::FlushEvents),
            // log/metric were fully implemented (VM impl, BuiltinSig, IO
            // effect row) but missing from this table, which made them
            // unreachable — "Undefined variable" (dogfood table-parity audit).
            "log" => Some(Builtin::Log),
            "metric" => Some(Builtin::Metric),
            "byte_at" => Some(Builtin::ByteAt),
            "substring_bytes" => Some(Builtin::SubstringBytes),
            "byte_len" => Some(Builtin::ByteLen),
            "read_file" => Some(Builtin::ReadFile),
            "write_file" => Some(Builtin::WriteFile),
            "http_get" => Some(Builtin::HttpGet),
            "regex_is_match" => Some(Builtin::RegexIsMatch),
            "regex_find" => Some(Builtin::RegexFind),
            "now_unix_s" => Some(Builtin::NowUnixS),
            "now_unix_ms" => Some(Builtin::NowUnixMs),
            "rand_int" => Some(Builtin::RandInt),
            "rand_float" => Some(Builtin::RandFloat),
            "rand_bool" => Some(Builtin::RandBool),
            "rand_seed" => Some(Builtin::RandSeed),
            "gen_int" => Some(Builtin::GenInt),
            "gen_float" => Some(Builtin::GenFloat),
            "gen_str" => Some(Builtin::GenStr),
            "gen_bool" => Some(Builtin::GenBool),
            "gen_list" => Some(Builtin::GenList),
            "input" => Some(Builtin::Input),
            "readline" => Some(Builtin::Readline),
            "assert" => Some(Builtin::Assert),
            "assert_eq" => Some(Builtin::AssertEq),
            "sort_by" => Some(Builtin::SortBy),
            "unwrap_or" => Some(Builtin::UnwrapOr),
            "map_or" => Some(Builtin::MapOr),
            "is_some" => Some(Builtin::IsSome),
            "is_none" => Some(Builtin::IsNone),
            "load_extension" => Some(Builtin::LoadExtension),
            "gc_collect" => Some(Builtin::GcCollect),
            "eprint" => Some(Builtin::Eprint),
            "write_stdout" => Some(Builtin::WriteStdout),
            "write_stderr" => Some(Builtin::WriteStderr),
            "read_stdin_all" => Some(Builtin::ReadStdinAll),
            "flush_stdout" => Some(Builtin::FlushStdout),
            "sleep_ms" => Some(Builtin::SleepMs),
            "append_file" => Some(Builtin::AppendFile),
            "file_exists" => Some(Builtin::FileExists),
            "remove_file" => Some(Builtin::RemoveFile),
            "list_dir" => Some(Builtin::ListDir),
            "create_dir" => Some(Builtin::CreateDir),
            "remove_dir" => Some(Builtin::RemoveDir),
            "read_file_bytes" => Some(Builtin::ReadFileBytes),
            "write_file_bytes" => Some(Builtin::WriteFileBytes),
            "http_post" => Some(Builtin::HttpPost),
            "http_post_json" => Some(Builtin::HttpPostJson),
            "http_request" => Some(Builtin::HttpRequest),
            "tcp_connect" => Some(Builtin::TcpConnect),
            "tcp_listen" => Some(Builtin::TcpListen),
            "tcp_accept" => Some(Builtin::TcpAccept),
            "tcp_accept_timeout" => Some(Builtin::TcpAcceptTimeout),
            "tcp_read" => Some(Builtin::TcpRead),
            "tcp_write" => Some(Builtin::TcpWrite),
            "tcp_close" => Some(Builtin::TcpClose),
            "udp_bind" => Some(Builtin::UdpBind),
            "udp_recv_from" => Some(Builtin::UdpRecvFrom),
            "udp_recv_from_timeout" => Some(Builtin::UdpRecvFromTimeout),
            "udp_recv_from_bytes" => Some(Builtin::UdpRecvFromBytes),
            "udp_recv_from_bytes_timeout" => Some(Builtin::UdpRecvFromBytesTimeout),
            "udp_recv_bytebuf" => Some(Builtin::UdpRecvByteBuf),
            "udp_recv_bytebuf_timeout" => Some(Builtin::UdpRecvByteBufTimeout),
            "udp_send_to" => Some(Builtin::UdpSendTo),
            "udp_send_to_bytes" => Some(Builtin::UdpSendToBytes),
            "udp_send_bytebuf" => Some(Builtin::UdpSendByteBuf),
            "udp_close" => Some(Builtin::UdpClose),
            "query_where" => Some(Builtin::QueryWhere),
            "query_map" => Some(Builtin::QueryMap),
            "query_count" => Some(Builtin::QueryCount),
            "with_field" => Some(Builtin::WithField),
            "bitset_new" => Some(Builtin::BitsetNew),
            "bitset_set" => Some(Builtin::BitsetSet),
            "bitset_has" => Some(Builtin::BitsetHas),
            "bitset_clear" => Some(Builtin::BitsetClear),
            "buffer_new" => Some(Builtin::BufferNew),
            "buffer_append" => Some(Builtin::BufferAppend),
            "buffer_to_str" => Some(Builtin::BufferToStr),
            "bytebuf_new" => Some(Builtin::ByteBufNew),
            "bytebuf_len" => Some(Builtin::ByteBufLen),
            "bytebuf_get" => Some(Builtin::ByteBufGet),
            "bytebuf_set_u8" => Some(Builtin::ByteBufSetU8),
            "bytebuf_set_u32_le" => Some(Builtin::ByteBufSetU32Le),
            "bytebuf_set_i32_le" => Some(Builtin::ByteBufSetI32Le),
            "bytebuf_get_u32_le" => Some(Builtin::ByteBufGetU32Le),
            "bytebuf_get_i32_le" => Some(Builtin::ByteBufGetI32Le),
            "bytebuf_to_list" => Some(Builtin::ByteBufToList),
            "bytebuf_from_list" => Some(Builtin::ByteBufFromList),
            "fork" => Some(Builtin::Fork),
            "simulate" => Some(Builtin::Simulate),
            "commit" => Some(Builtin::Commit),
            "clock" => Some(Builtin::Clock),
            "peek" => Some(Builtin::Peek),
            "peek_resource" => Some(Builtin::PeekResource),
            "debug_trace" => Some(Builtin::DebugTrace),
            "format_value" => Some(Builtin::FormatValue),
            "round" => Some(Builtin::Round),
            "floor" => Some(Builtin::Floor),
            "ceil" => Some(Builtin::Ceil),
            "sqrt" => Some(Builtin::Sqrt),
            "pow" => Some(Builtin::Pow),
            "to_fixed" => Some(Builtin::ToFixed),
            "json_stringify" => Some(Builtin::JsonStringify),
            "json_parse" => Some(Builtin::JsonParse),
            "simulate_par" => Some(Builtin::SimulatePar),
            "simulate_many" => Some(Builtin::SimulateMany),
            "simulate_seeded" => Some(Builtin::SimulateSeeded),
            "fork_with" => Some(Builtin::ForkWith),
            "fork_seed" => Some(Builtin::ForkSeed),
            "sandbox_run" => Some(Builtin::SandboxRun),
            "sandbox_input" => Some(Builtin::SandboxInput),
            "sandbox_output" => Some(Builtin::SandboxOutput),
            "sandbox_last_output" => Some(Builtin::SandboxLastOutput),
            "sandbox_last_fuel" => Some(Builtin::SandboxLastFuel),
            "diff" => Some(Builtin::Diff),
            "assert_only_changed" => Some(Builtin::AssertOnlyChanged),
            "why" => Some(Builtin::Why),
            "why_resource" => Some(Builtin::WhyResource),
            "save_world" => Some(Builtin::SaveWorld),
            "load_world" => Some(Builtin::LoadWorld),
            "try_load_world" => Some(Builtin::TryLoadWorld),
            "world_digest" => Some(Builtin::WorldDigest),
            "schema_digest" => Some(Builtin::SchemaDigest),
            "merge_forks" => Some(Builtin::MergeForks),
            "merge_forks_with" => Some(Builtin::MergeForksWith),
            "fork_to_bytes" => Some(Builtin::ForkToBytes),
            "fork_from_bytes" => Some(Builtin::ForkFromBytes),
            "fork_delta" => Some(Builtin::ForkDelta),
            "fork_apply" => Some(Builtin::ForkApply),
            "base_fact" => Some(Builtin::BaseFact),
            "candidate_fact" => Some(Builtin::CandidateFact),
            "insert_fact" => Some(Builtin::InsertFact),
            "remove_fact" => Some(Builtin::RemoveFact),
            "replace_fact_by" => Some(Builtin::ReplaceFactBy),
            _ => None,
        }
    }

    pub fn return_type(self) -> Ty {
        match self {
            Builtin::Print
            | Builtin::Set
            | Builtin::SetResource
            | Builtin::InsertFact
            | Builtin::RemoveFact
            | Builtin::ReplaceFactBy => Ty::Nil,
            Builtin::Len | Builtin::Int | Builtin::IntDiv | Builtin::GcCollect => Ty::Int,
            Builtin::Popcount | Builtin::Ctz | Builtin::Shl | Builtin::Shr => Ty::Int,
            Builtin::IdOf => Ty::Int,
            Builtin::Filled | Builtin::SetAt => Ty::List(Box::new(Ty::Any)),
            Builtin::Sum | Builtin::Product | Builtin::GetOr | Builtin::Clamp => Ty::Any,
            Builtin::IndexOf => Ty::Int,
            Builtin::Any | Builtin::All => Ty::Bool,
            Builtin::Abs => Ty::Any,
            Builtin::Sign => Ty::Any,
            Builtin::Float => Ty::Float,
            Builtin::TypeOf
            | Builtin::Str
            | Builtin::Input
            | Builtin::Readline
            | Builtin::NameOf => Ty::Str,
            Builtin::Format => Ty::Str,
            Builtin::Entries => Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            Builtin::Merge => Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any)),
            Builtin::RemoveKey => Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any)),
            // keys come from the key_fn: str, int, tuple — not just str
            Builtin::GroupBy => Ty::Map(Box::new(Ty::Any), Box::new(Ty::List(Box::new(Ty::Any)))),
            Builtin::Min
            | Builtin::Max
            | Builtin::Reduce
            | Builtin::Unwrap
            | Builtin::Expect
            | Builtin::UnwrapOr
            | Builtin::MapOr
            | Builtin::LoadExtension => Ty::Any,
            Builtin::Push
            | Builtin::Reverse
            | Builtin::Sort
            | Builtin::SortBy
            | Builtin::Filter
            | Builtin::Map
            | Builtin::DropLast
            | Builtin::DropFirst
            | Builtin::RecentEvents
            | Builtin::Slice => Ty::List(Box::new(Ty::Any)),
            Builtin::Pop | Builtin::PopLast => Ty::Any,
            Builtin::Range => Ty::List(Box::new(Ty::Int)),
            Builtin::Keys => Ty::List(Box::new(Ty::Any)),
            Builtin::Contains
            | Builtin::Has
            | Builtin::BaseFact
            | Builtin::CandidateFact
            | Builtin::Remove
            | Builtin::Despawn
            | Builtin::StartsWith
            | Builtin::EndsWith
            | Builtin::IsSome
            | Builtin::IsNone
            | Builtin::BitsetHas => Ty::Bool,
            Builtin::Split => Ty::List(Box::new(Ty::Str)),
            Builtin::Join | Builtin::Trim | Builtin::Replace => Ty::Str,
            Builtin::Append
            | Builtin::Extend
            | Builtin::Zip
            | Builtin::FlatMap
            | Builtin::Enumerate => Ty::List(Box::new(Ty::Any)),
            Builtin::TryInt
            | Builtin::TryFloat
            | Builtin::Find
            | Builtin::MaxBy
            | Builtin::MinBy => Ty::SumType("Option".to_string()),
            Builtin::Get | Builtin::GetResource => Ty::SumType("Option".to_string()),
            Builtin::Res => Ty::Any,
            Builtin::Lookup => Ty::SumType("Option".to_string()),
            Builtin::LookupAll => Ty::List(Box::new(Ty::EntityId)),
            Builtin::Require => Ty::Any,
            Builtin::RequireAll => Ty::List(Box::new(Ty::Any)),
            Builtin::Transition => Ty::SumType("Result".to_string()),
            Builtin::Spawn => Ty::EntityId,
            Builtin::GetEntity => Ty::SumType("Option".to_string()),
            Builtin::RequireEntity => Ty::EntityId,
            Builtin::Entities => Ty::List(Box::new(Ty::EntityId)),
            Builtin::Chr | Builtin::ToUpper | Builtin::ToLower | Builtin::SubstringBytes => Ty::Str,
            Builtin::Ord | Builtin::ByteAt | Builtin::ByteLen => Ty::Int,
            Builtin::Chars => Ty::List(Box::new(Ty::Str)),
            Builtin::Values => Ty::List(Box::new(Ty::Any)),
            Builtin::ReadFile | Builtin::HttpGet => Ty::Str,
            Builtin::WriteFile => Ty::Nil,
            Builtin::RegexIsMatch => Ty::Bool,
            Builtin::RegexFind => Ty::SumType("Option".to_string()),
            Builtin::NowUnixS | Builtin::NowUnixMs => Ty::Int,
            Builtin::Round | Builtin::Floor | Builtin::Ceil => Ty::Int,
            Builtin::Sqrt => Ty::Float,
            Builtin::Pow => Ty::Any,
            Builtin::ToFixed | Builtin::JsonStringify => Ty::Str,
            Builtin::JsonParse => Ty::SumType("Option".to_string()),
            Builtin::SimulatePar => Ty::List(Box::new(Ty::WorldFork)),
            Builtin::SimulateMany => Ty::List(Box::new(Ty::WorldFork)),
            Builtin::SimulateSeeded => Ty::WorldFork,
            Builtin::ForkWith => Ty::WorldFork,
            Builtin::ForkSeed => Ty::Int,
            Builtin::SandboxRun => Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str]),
            Builtin::SandboxInput => Ty::Any,
            Builtin::SandboxOutput => Ty::Nil,
            Builtin::SandboxLastOutput => Ty::Any,
            Builtin::SandboxLastFuel => Ty::Int,
            Builtin::Diff => Ty::Map(Box::new(Ty::Str), Box::new(Ty::Int)),
            Builtin::AssertOnlyChanged => Ty::Nil,
            Builtin::Why | Builtin::WhyResource => Ty::Str,
            Builtin::SaveWorld => Ty::Str,
            Builtin::WorldDigest => Ty::Str,
            Builtin::SchemaDigest => Ty::Str,
            Builtin::LoadWorld => Ty::Int,
            Builtin::TryLoadWorld => Ty::App("Result".to_string(), vec![Ty::Int, Ty::Str]),
            Builtin::MergeForks | Builtin::MergeForksWith => Ty::App(
                "Result".to_string(),
                vec![
                    Ty::WorldFork,
                    Ty::List(Box::new(Ty::SumType("Conflict".to_string()))),
                ],
            ),
            Builtin::ForkToBytes | Builtin::ForkDelta => Ty::Str,
            Builtin::ForkFromBytes | Builtin::ForkApply => {
                Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str])
            }
            Builtin::RandInt => Ty::Int,
            Builtin::RandFloat => Ty::Float,
            Builtin::RandBool => Ty::Bool,
            Builtin::RandSeed => Ty::Nil,
            Builtin::GenInt => Ty::List(Box::new(Ty::Int)),
            Builtin::GenFloat => Ty::List(Box::new(Ty::Float)),
            Builtin::GenStr => Ty::List(Box::new(Ty::Str)),
            Builtin::GenBool => Ty::List(Box::new(Ty::Bool)),
            Builtin::GenList => Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            Builtin::Assert | Builtin::AssertEq => Ty::Nil,
            Builtin::Eprint
            | Builtin::WriteStdout
            | Builtin::WriteStderr
            | Builtin::FlushStdout
            | Builtin::SleepMs
            | Builtin::AppendFile
            | Builtin::RemoveFile
            | Builtin::CreateDir
            | Builtin::RemoveDir
            | Builtin::WriteFileBytes
            | Builtin::TcpWrite
            | Builtin::TcpClose
            | Builtin::UdpClose => Ty::Nil,
            Builtin::ReadStdinAll
            | Builtin::HttpPost
            | Builtin::HttpPostJson
            | Builtin::TcpRead => Ty::Str,
            Builtin::FileExists => Ty::Bool,
            Builtin::ListDir => Ty::List(Box::new(Ty::Str)),
            Builtin::ReadFileBytes => Ty::List(Box::new(Ty::Int)),
            Builtin::HttpRequest => Ty::Map(Box::new(Ty::Str), Box::new(Ty::Any)),
            Builtin::TcpAcceptTimeout
            | Builtin::UdpRecvFromTimeout
            | Builtin::UdpRecvFromBytesTimeout
            | Builtin::UdpRecvByteBufTimeout => Ty::SumType("Option".to_string()),
            Builtin::UdpRecvFrom => Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Int]),
            Builtin::UdpRecvFromBytes => {
                Ty::Tuple(vec![Ty::List(Box::new(Ty::Int)), Ty::Str, Ty::Int])
            }
            Builtin::UdpRecvByteBuf => Ty::Tuple(vec![Ty::Any, Ty::Str, Ty::Int]),
            Builtin::TcpConnect
            | Builtin::TcpListen
            | Builtin::TcpAccept
            | Builtin::UdpBind
            | Builtin::UdpSendTo
            | Builtin::UdpSendToBytes
            | Builtin::UdpSendByteBuf => Ty::Int,
            Builtin::QueryWhere | Builtin::WithField => Ty::List(Box::new(Ty::EntityId)),
            Builtin::QueryMap => Ty::List(Box::new(Ty::Any)),
            Builtin::QueryCount => Ty::Int,
            Builtin::VariantOf => Ty::Str,
            Builtin::SysArgs => Ty::List(Box::new(Ty::Str)),
            Builtin::BitsetNew | Builtin::BitsetSet | Builtin::BitsetClear => Ty::BitSet,
            Builtin::BufferNew => Ty::Any,
            Builtin::BufferAppend => Ty::Any,
            Builtin::BufferToStr => Ty::Str,
            Builtin::ByteBufNew
            | Builtin::ByteBufSetU8
            | Builtin::ByteBufSetU32Le
            | Builtin::ByteBufSetI32Le
            | Builtin::ByteBufFromList => Ty::Any,
            Builtin::ByteBufLen
            | Builtin::ByteBufGet
            | Builtin::ByteBufGetU32Le
            | Builtin::ByteBufGetI32Le => Ty::Int,
            Builtin::ByteBufToList => Ty::List(Box::new(Ty::Int)),
            Builtin::Log | Builtin::Metric => Ty::Nil,
            Builtin::TraceId => Ty::Any,
            Builtin::Fork => Ty::WorldFork,
            Builtin::Simulate => Ty::WorldFork,
            Builtin::Commit => Ty::Nil,
            Builtin::Clock => Ty::Float,
            Builtin::Peek => Ty::App("Option".to_string(), vec![Ty::Any]),
            Builtin::PeekResource => Ty::App("Option".to_string(), vec![Ty::Any]),
            Builtin::FlushEvents => Ty::Nil,
            Builtin::DebugTrace => Ty::Any,
            Builtin::FormatValue => Ty::Str,
        }
    }
}
