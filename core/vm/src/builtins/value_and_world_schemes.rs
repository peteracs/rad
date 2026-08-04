

pub fn is_builtin(name: &str) -> bool {
    Builtin::from_name(name).is_some() || name == "emit"
}

pub struct BuiltinSig {
    pub type_params: Vec<String>,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub is_pure: bool,
}

impl BuiltinSig {
    pub fn effects(&self, name: &str) -> EffectSet {
        builtin_effect(name)
    }
}

pub fn builtin_effect(name: &str) -> EffectSet {
    match name {
        // rand_* joined this arm for table parity (they are IO in
        // builtin_required_effects and in is_impure_builtin): the sandbox
        // mask, this table's one functional consumer, carries an explicit
        // allow arm for them so seeded guest randomness keeps working.
        "print" | "now_unix_s" | "now_unix_ms" | "clock" | "load_extension" | "gc_collect"
        | "eprint" | "write_stdout" | "write_stderr" | "flush_stdout" | "sleep_ms" | "log"
        | "metric" | "rand_int" | "rand_float" | "rand_bool" | "rand_seed" => {
            EffectSet::single(Effect::IO)
        }
        "input"
        | "readline"
        | "read_file"
        | "write_file"
        | "http_get"
        | "read_stdin_all"
        | "append_file"
        | "file_exists"
        | "remove_file"
        | "list_dir"
        | "create_dir"
        | "remove_dir"
        | "read_file_bytes"
        | "write_file_bytes"
        | "http_post"
        | "http_post_json"
        | "http_request"
        | "tcp_connect"
        | "tcp_listen"
        | "tcp_accept"
        | "tcp_accept_timeout"
        | "tcp_read"
        | "tcp_write"
        | "tcp_close"
        | "udp_bind"
        | "udp_recv_from"
        | "udp_recv_from_timeout"
        | "udp_recv_from_bytes"
        | "udp_recv_from_bytes_timeout"
        | "udp_recv_bytebuf"
        | "udp_recv_bytebuf_timeout"
        | "udp_send_to"
        | "udp_send_to_bytes"
        | "udp_send_bytebuf"
        | "udp_close" => EffectSet::from_vec(&[Effect::IO, Effect::Async]),
        "set" | "set_resource" | "spawn" | "remove" | "despawn" | "fork" | "fork_with"
        | "simulate" | "simulate_par" | "simulate_many" | "simulate_seeded" | "sandbox_run"
        | "commit" | "load_world" | "try_load_world" | "merge_forks" | "merge_forks_with"
        | "fork_from_bytes" | "fork_apply" => EffectSet::single(Effect::ECS),
        "get" | "has" | "entities" | "query_where" | "query_map" | "query_count" | "with_field"
        | "peek" | "peek_resource" | "fork_seed" | "lookup" | "lookup_all" | "why"
        | "why_resource" | "save_world" | "name_of" | "require_entity" | "world_digest"
        | "schema_digest" | "res" | "get_resource" | "require" | "require_all" | "get_entity"
        | "recent_events" => EffectSet::single(Effect::ReadECS),
        "emit" | "transition" | "flush_events" => EffectSet::single(Effect::Event),
        _ => EffectSet::pure(),
    }
}

/// Human signature for an arity/argument error hint. Curated entries carry
/// the parameter *names* (names teach, types only describe) for the builtins
/// people actually stumble on; everything else falls back to a signature
/// generated from [`builtin_type_scheme`], which cannot rot.
pub fn builtin_signature_help(name: &str) -> Option<String> {
    let curated = match name {
        "simulate" => "simulate(fork, systems, ticks) -> world_fork",
        "simulate_par" => {
            "simulate_par(fork, systems, ticks, n_futures, seed, with?: list<resource>) -> list<world_fork>"
        }
        "simulate_many" => "simulate_many(forks, systems, ticks, seed) -> list<world_fork>",
        "simulate_seeded" => "simulate_seeded(fork, systems, ticks, raw_seed) -> world_fork",
        "fork_with" => "fork_with(fork, resource_value) -> world_fork",
        "fork_seed" => "fork_seed(fork) -> int",
        "merge_forks" => "merge_forks(base, ours, theirs) -> Result<world_fork, list<Conflict>>",
        "merge_forks_with" => {
            "merge_forks_with(base, ours, theirs, resolutions: list<(Conflict, value)>) -> Result<world_fork, list<Conflict>>"
        }
        "fork_delta" => "fork_delta(base, fork) -> str",
        "fork_apply" => "fork_apply(base, delta) -> Result<world_fork, str>",
        "slice" => "slice(list_or_str, start, end) -> list_or_str",
        "range" => "range(start, end) -> list<int>",
        "map" => "map(list, fn(item)) -> list",
        "filter" => "filter(list, fn(item) -> bool) -> list",
        "reduce" => "reduce(list, init, fn(acc, item)) -> value",
        "spawn" => "spawn(name?, Component { .. }, ...) -> entity",
        "get" => "get(entity, Component) -> Option<Component>",
        "peek" => "peek(fork, entity, Component) -> Option<Component>",
        "require" => "require(entity, Component) -> Component (runtime error if missing)",
        "expect" => "expect(option_or_result, message) -> value",
        "unwrap_or" => "unwrap_or(option_or_result, default) -> value",
        "rand_int" => "rand_int(min, max) -> int",
        "replace" => "replace(s, from, to) -> str",
        "split" => "split(s, separator) -> list<str>",
        "join" => "join(list, separator) -> str",
        "sandbox_run" => {
            "sandbox_run(source, fork, caps_json, input?) -> Result<world_fork, str>"
        }
        "assert_only_changed" => "assert_only_changed(before, after, allowed: list<Component>) -> nil",
        _ => "",
    };
    if !curated.is_empty() {
        return Some(curated.to_string());
    }
    let sig = builtin_type_scheme(name)?;
    let params: Vec<String> = sig.params.iter().map(|t| format!("{}", t)).collect();
    Some(format!("{}({}) -> {}", name, params.join(", "), sig.ret))
}

pub fn builtin_type_scheme(name: &str) -> Option<BuiltinSig> {
    match name {
        "len" | "typeof" | "variant_of" | "sys_args" | "str" | "int" | "int_div" | "float" | "abs" | "sign" | "popcount" | "ctz" | "shl" | "shr" | "filled" | "set_at" | "sum" | "product" | "get_or" | "clamp" | "index_of" | "any" | "all" | "min" | "max" | "log" | "metric" | "trace_id" | "flush_events" | "print" | "debug_trace" | "pop" | "pop_last" | "drop_last" | "drop_first" | "recent_events" | "sort" | "reverse" | "sort_by" | "slice" | "map" | "filter" | "reduce" | "range" | "keys" | "contains" | "format" | "format_value" | "entries" | "merge" | "remove_key" | "group_by" | "split" | "join" | "trim" | "replace" | "starts_with" | "ends_with" | "append" | "extend" | "zip" | "flat_map" | "enumerate" | "find" | "max_by" | "min_by" | "try_int" | "try_float" => builtin_type_scheme_values(name),
        "get" | "res" | "get_resource" | "set_resource" | "lookup" | "lookup_all" | "require" | "get_entity" | "require_entity" | "require_all" | "set" | "has" | "spawn" | "remove" | "despawn" | "name_of" | "id_of" | "entities" | "transition" | "emit" | "unwrap" | "expect" | "unwrap_or" | "map_or" | "is_some" | "is_none" | "chr" | "ord" | "chars" | "to_upper" | "to_lower" | "values" | "byte_at" | "substring_bytes" | "byte_len" => builtin_type_scheme_world(name),
        "read_file" | "write_file" | "http_get" | "regex_is_match" | "regex_find" | "now_unix_s" | "now_unix_ms" | "round" | "floor" | "ceil" | "sqrt" | "pow" | "to_fixed" | "json_stringify" | "json_parse" | "rand_int" | "rand_float" | "rand_bool" | "rand_seed" | "gen_int" | "gen_float" | "gen_str" | "gen_bool" | "gen_list" | "input" | "readline" | "assert" | "assert_eq" | "load_extension" | "gc_collect" | "eprint" | "write_stdout" | "write_stderr" | "read_stdin_all" | "flush_stdout" | "sleep_ms" | "append_file" | "file_exists" | "remove_file" | "list_dir" | "create_dir" | "remove_dir" | "read_file_bytes" | "write_file_bytes" | "http_post" | "http_post_json" | "http_request" | "tcp_connect" | "tcp_listen" | "tcp_accept" | "tcp_accept_timeout" | "tcp_read" | "tcp_write" | "tcp_close" | "udp_bind" | "udp_recv_from" | "udp_recv_from_timeout" | "udp_recv_from_bytes" | "udp_recv_from_bytes_timeout" | "udp_recv_bytebuf" | "udp_recv_bytebuf_timeout" | "udp_send_to" | "udp_send_to_bytes" | "udp_send_bytebuf" | "udp_close" => builtin_type_scheme_host(name),
        "query_where" | "query_map" | "query_count" | "with_field" | "bitset_new" | "bitset_set" | "bitset_has" | "bitset_clear" | "buffer_new" | "buffer_append" | "buffer_to_str" | "bytebuf_new" | "bytebuf_len" | "bytebuf_get" | "bytebuf_set_u8" | "bytebuf_set_u32_le" | "bytebuf_set_i32_le" | "bytebuf_get_u32_le" | "bytebuf_get_i32_le" | "bytebuf_to_list" | "bytebuf_from_list" => builtin_type_scheme_buffers(name),
        "fork" | "simulate_par" | "simulate_many" | "simulate_seeded" | "fork_with" | "fork_seed" | "sandbox_run" | "sandbox_input" | "sandbox_output" | "sandbox_last_output" | "sandbox_last_fuel" | "diff" | "assert_only_changed" | "why" | "why_resource" | "save_world" | "load_world" | "try_load_world" | "world_digest" | "schema_digest" | "fork_to_bytes" | "fork_from_bytes" | "fork_delta" | "fork_apply" | "merge_forks" | "merge_forks_with" | "simulate" | "commit" | "clock" | "peek" | "peek_resource" => builtin_type_scheme_simulation(name),
        _ => None,
    }
}

fn builtin_type_scheme_values(name: &str) -> Option<BuiltinSig> {
    let a = || Ty::App("A".to_string(), vec![]);
    let b = || Ty::App("B".to_string(), vec![]);
    let tp_a = || vec!["A".to_string()];
    let tp_ab = || vec!["A".to_string(), "B".to_string()];

    let sig = match name {
            "len" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Int,
            is_pure: true,
        },
        "typeof" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Str,
            is_pure: true,
        },
        "variant_of" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        "sys_args" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::Str)),
            is_pure: false,
        },
        "str" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Str,
            is_pure: true,
        },
        "int" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Int,
            is_pure: true,
        },
        "int_div" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            is_pure: true,
        },
        "float" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Float,
            is_pure: true,
        },
        "abs" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        // -1/0/1, int-preserving (float in, float out) — Math.sign
        "sign" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        "popcount" | "ctz" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Int,
            is_pure: true,
        },
        "shl" | "shr" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            is_pure: true,
        },
        "filled" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Any],
            ret: Ty::List(Box::new(Ty::Any)),
            is_pure: true,
        },
        "set_at" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::List(Box::new(Ty::Any)), Ty::Int, Ty::Any],
            ret: Ty::List(Box::new(Ty::Any)),
            is_pure: true,
        },
        "sum" | "product" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::List(Box::new(Ty::Any))],
            ret: Ty::Any,
            is_pure: true,
        },
        "get_or" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Any, Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        "clamp" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Any, Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        "index_of" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::List(Box::new(Ty::Any)), Ty::Any],
            ret: Ty::Int,
            is_pure: true,
        },
        "any" | "all" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::List(Box::new(Ty::Any)),
                Ty::Fn {
                    params: vec![Ty::Any],
                    ret: Box::new(Ty::Bool),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::Bool,
            is_pure: true,
        },
        "min" | "max" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        "log" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Map(Box::new(Ty::Str), Box::new(Ty::Any))],
            ret: Ty::Nil,
            is_pure: false,
        },
        "metric" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::Str,
                Ty::Str,
                Ty::Float,
                Ty::Map(Box::new(Ty::Str), Box::new(Ty::Any)),
            ],
            ret: Ty::Nil,
            is_pure: false,
        },
        "trace_id" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Int,
            is_pure: true,
        },
        "flush_events" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Nil,
            is_pure: false,
        },
        "print" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Nil,
            is_pure: false,
        },

        "debug_trace" => BuiltinSig {
            type_params: tp_a(),
            params: vec![a()],
            ret: a(),
            is_pure: true,
        },
        "pop" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: a(),
            is_pure: true,
        },
        "pop_last" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: a(),
            is_pure: true,
        },
        "drop_last" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "drop_first" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        // Reads the dispatched-event log (a world-history read, like get()).
        "recent_events" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Int],
            ret: Ty::List(Box::new(Ty::Any)),
            is_pure: false,
        },
        "sort" | "reverse" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "sort_by" => BuiltinSig {
            type_params: tp_a(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(Ty::Any),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "slice" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a())), Ty::Int, Ty::Int],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "map" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(b()),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::List(Box::new(b())),
            is_pure: true,
        },
        "filter" => BuiltinSig {
            type_params: tp_a(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(Ty::Bool),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "reduce" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![
                Ty::List(Box::new(a())),
                b(),
                Ty::Fn {
                    params: vec![b(), a()],
                    ret: Box::new(b()),
                    purity: FnPurity::Impure,
                },
            ],
            ret: b(),
            is_pure: true,
        },
        "range" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int, Ty::Int],
            ret: Ty::List(Box::new(Ty::Int)),
            is_pure: true,
        },
        "keys" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![Ty::Map(Box::new(a()), Box::new(b()))],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "contains" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a())), a()],
            ret: Ty::Bool,
            is_pure: true,
        },
        "format" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "format_value" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "entries" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![Ty::Map(Box::new(a()), Box::new(b()))],
            ret: Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            is_pure: true,
        },
        "merge" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![
                Ty::Map(Box::new(a()), Box::new(b())),
                Ty::Map(Box::new(a()), Box::new(b())),
            ],
            ret: Ty::Map(Box::new(a()), Box::new(b())),
            is_pure: true,
        },
        "remove_key" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![Ty::Map(Box::new(a()), Box::new(b())), a()],
            ret: Ty::Map(Box::new(a()), Box::new(b())),
            is_pure: true,
        },
        "group_by" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![
                Ty::List(Box::new(a())),
                // key_fn returns any valid map key: str, int, tuple…
                // readonly closures allowed (same as map/filter/sort_by)
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(b()),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::Map(Box::new(b()), Box::new(Ty::List(Box::new(a())))),
            is_pure: true,
        },
        "split" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::List(Box::new(Ty::Str)),
            is_pure: true,
        },
        "join" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a())), Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "trim" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "replace" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str, Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "starts_with" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Bool,
            is_pure: true,
        },
        "ends_with" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Bool,
            is_pure: true,
        },
        "append" | "extend" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a())), Ty::List(Box::new(a()))],
            ret: Ty::List(Box::new(a())),
            is_pure: true,
        },
        "zip" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![Ty::List(Box::new(a())), Ty::List(Box::new(b()))],
            ret: Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            is_pure: true,
        },
        "flat_map" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(Ty::List(Box::new(b()))),
                    purity: FnPurity::Pure,
                },
            ],
            ret: Ty::List(Box::new(b())),
            is_pure: true,
        },
        "enumerate" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            is_pure: true,
        },
        "find" => BuiltinSig {
            type_params: tp_a(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(Ty::Bool),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::App("Option".to_string(), vec![a()]),
            is_pure: true,
        },
        "max_by" => BuiltinSig {
            type_params: tp_a(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(Ty::Any),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::App("Option".to_string(), vec![a()]),
            is_pure: true,
        },
        "min_by" => BuiltinSig {
            type_params: tp_a(),
            params: vec![
                Ty::List(Box::new(a())),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(Ty::Any),
                    purity: FnPurity::Impure,
                },
            ],
            ret: Ty::App("Option".to_string(), vec![a()]),
            is_pure: true,
        },
        "try_int" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::Int]),
            is_pure: true,
        },
        "try_float" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::Float]),
            is_pure: true,
        },
            _ => return None,
        };
    Some(sig)
}

fn builtin_type_scheme_world(name: &str) -> Option<BuiltinSig> {
    let a = || Ty::App("A".to_string(), vec![]);
    let b = || Ty::App("B".to_string(), vec![]);
    let tp_a = || vec!["A".to_string()];
    let tp_ab = || vec!["A".to_string(), "B".to_string()];

    let sig = match name {
            "get" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::Any]),
            is_pure: false,
        },
        // res(R): direct resource read. Not pure (reads world state), but
        // readonly — allowed in `readonly fn` and rejected in `pure fn`,
        // exactly like get().
        "res" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Any,
            is_pure: false,
        },
        // get_resource(R): Option-shaped resource read; classified readonly
        // exactly like res(). Was scheme-less, which defaulted it to a pure
        // 0-arity stub in first-class position.
        "get_resource" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::Any]),
            is_pure: false,
        },
        // set_resource(R, value): the resource-level ECS write. The missing
        // scheme left first-class `set_resource` typed as a bogus pure-ish
        // 0-arity stub (dogfood soundness follow-up to seq 253); call-site
        // resource/system validation stays in check_typed_ecs_builtin.
        "set_resource" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Any],
            ret: Ty::Nil,
            is_pure: false,
        },
        "lookup" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Str, Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::EntityId]),
            is_pure: false,
        },
        "lookup_all" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Str, Ty::Any],
            ret: Ty::List(Box::new(Ty::EntityId)),
            is_pure: false,
        },
        "require" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::Any,
            is_pure: false,
        },
        // World read (the name table): not pure, readonly — like get().
        // Returns nil when the name is unbound; typed Any (not the honest
        // `entity | nil`) until callers migrate to guard-narrowing.
        // honest type: a lookup by name can miss. Guard-clause narrowing
        // (`if e == nil { return }`) recovers the bare entity.
        "get_entity" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Union(vec![Ty::EntityId, Ty::Nil]),
            is_pure: false,
        },
        // the fail-fast dual (get/require, extended to name lookup)
        "require_entity" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::EntityId,
            is_pure: false,
        },
        "require_all" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::List(Box::new(Ty::Any)),
            is_pure: false,
        },
        "set" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::Nil,
            is_pure: false,
        },
        "has" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::Bool,
            is_pure: false,
        },
        "spawn" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::EntityId,
            is_pure: false,
        },
        "remove" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::Bool,
            is_pure: false,
        },
        "despawn" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId],
            ret: Ty::Bool,
            is_pure: false,
        },
        "name_of" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId],
            ret: Ty::Str,
            is_pure: false,
        },
        // pure: the id is carried by the value itself, no world read
        "id_of" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId],
            ret: Ty::Int,
            is_pure: true,
        },
        "entities" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::EntityId)),
            is_pure: false,
        },
        "transition" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Str],
            ret: Ty::App("Result".to_string(), vec![Ty::Any, Ty::Str]),
            is_pure: false,
        },
        "emit" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Nil,
            is_pure: false,
        },
        "unwrap" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::App("Option".to_string(), vec![a()])],
            ret: a(),
            is_pure: true,
        },
        "expect" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::App("Option".to_string(), vec![a()]), Ty::Str],
            ret: a(),
            is_pure: true,
        },
        "unwrap_or" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::App("Option".to_string(), vec![a()]), a()],
            ret: a(),
            is_pure: true,
        },
        "map_or" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![
                Ty::Any,
                b(),
                Ty::Fn {
                    params: vec![a()],
                    ret: Box::new(b()),
                    purity: FnPurity::Impure,
                },
            ],
            ret: b(),
            is_pure: true,
        },
        "is_some" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Bool,
            is_pure: true,
        },
        "is_none" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Bool,
            is_pure: true,
        },
        "chr" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Str,
            is_pure: true,
        },
        "ord" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Int,
            is_pure: true,
        },
        "chars" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::List(Box::new(Ty::Str)),
            is_pure: true,
        },
        "to_upper" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "to_lower" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Str,
            is_pure: true,
        },
        "values" => BuiltinSig {
            type_params: tp_ab(),
            params: vec![Ty::Map(Box::new(a()), Box::new(b()))],
            ret: Ty::List(Box::new(b())),
            is_pure: true,
        },
        "byte_at" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Int],
            ret: Ty::Int,
            is_pure: true,
        },
        "substring_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Int, Ty::Int],
            ret: Ty::Str,
            is_pure: true,
        },
        "byte_len" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Int,
            is_pure: true,
        },
            _ => return None,
        };
    Some(sig)
}
