use crate::types::{Effect, EffectSet, FnPurity, Ty};
use crate::value::Builtin;

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
        "read_file" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Str,
            is_pure: false,
        },
        "write_file" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "http_get" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Str,
            is_pure: false,
        },
        "regex_is_match" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Bool,
            is_pure: true,
        },
        "regex_find" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::App("Option".to_string(), vec![Ty::Str]),
            is_pure: true,
        },
        "now_unix_s" | "now_unix_ms" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Int,
            is_pure: false,
        },
        "round" | "floor" | "ceil" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Int,
            is_pure: true,
        },
        "sqrt" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Float,
            is_pure: true,
        },
        "pow" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Any],
            ret: Ty::Any,
            is_pure: true,
        },
        "to_fixed" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Int],
            ret: Ty::Str,
            is_pure: true,
        },
        "json_stringify" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Str,
            is_pure: true,
        },
        "json_parse" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::App("Option".to_string(), vec![Ty::Any]),
            is_pure: true,
        },
        "rand_int" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Int,
            is_pure: false,
        },
        "rand_float" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Float,
            is_pure: false,
        },
        "rand_bool" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Bool,
            is_pure: false,
        },
        "rand_seed" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Nil,
            is_pure: false,
        },
        "gen_int" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::Int)),
            is_pure: true,
        },
        "gen_float" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::Float)),
            is_pure: true,
        },
        "gen_str" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::Str)),
            is_pure: true,
        },
        "gen_bool" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::Bool)),
            is_pure: true,
        },
        "gen_list" => BuiltinSig {
            type_params: tp_a(),
            params: vec![Ty::List(Box::new(a()))],
            ret: Ty::List(Box::new(Ty::List(Box::new(a())))),
            is_pure: true,
        },
        "input" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Str,
            is_pure: false,
        },
        "readline" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Str,
            is_pure: false,
        },
        "assert" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Bool, Ty::Str],
            ret: Ty::Nil,
            is_pure: true,
        },
        "assert_eq" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Any],
            ret: Ty::Nil,
            is_pure: true,
        },
        "load_extension" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Any,
            is_pure: false,
        },
        "gc_collect" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Int,
            is_pure: false,
        },
        "eprint" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Nil,
            is_pure: false,
        },
        "write_stdout" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "write_stderr" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "read_stdin_all" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Str,
            is_pure: false,
        },
        "flush_stdout" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Nil,
            is_pure: false,
        },
        "sleep_ms" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Nil,
            is_pure: false,
        },
        "append_file" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "file_exists" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Bool,
            is_pure: false,
        },
        "remove_file" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "list_dir" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::List(Box::new(Ty::Str)),
            is_pure: false,
        },
        "create_dir" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "remove_dir" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "read_file_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::List(Box::new(Ty::Int)),
            is_pure: false,
        },
        "write_file_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::List(Box::new(Ty::Int))],
            ret: Ty::Nil,
            is_pure: false,
        },
        "http_post" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Str,
            is_pure: false,
        },
        "http_post_json" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Str],
            ret: Ty::Str,
            is_pure: false,
        },
        "http_request" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::Str,
                Ty::Str,
                Ty::Map(Box::new(Ty::Str), Box::new(Ty::Str)),
                Ty::Str,
            ],
            ret: Ty::Map(Box::new(Ty::Str), Box::new(Ty::Any)),
            is_pure: false,
        },
        "tcp_connect" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Int],
            ret: Ty::Int,
            is_pure: false,
        },
        "tcp_listen" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Int],
            ret: Ty::Int,
            is_pure: false,
        },
        "tcp_accept" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Int,
            is_pure: false,
        },
        "tcp_accept_timeout" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::App("Option".to_string(), vec![Ty::Int]),
            is_pure: false,
        },
        "tcp_read" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Str,
            is_pure: false,
        },
        "tcp_write" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Str],
            ret: Ty::Nil,
            is_pure: false,
        },
        "tcp_close" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Nil,
            is_pure: false,
        },
        "udp_bind" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::Int],
            ret: Ty::Int,
            is_pure: false,
        },
        "udp_recv_from" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Int]),
            is_pure: false,
        },
        "udp_recv_from_timeout" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int, Ty::Int],
            ret: Ty::App(
                "Option".to_string(),
                vec![Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Int])],
            ),
            is_pure: false,
        },
        "udp_recv_from_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Tuple(vec![Ty::List(Box::new(Ty::Int)), Ty::Str, Ty::Int]),
            is_pure: false,
        },
        "udp_recv_from_bytes_timeout" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int, Ty::Int],
            ret: Ty::App(
                "Option".to_string(),
                vec![Ty::Tuple(vec![
                    Ty::List(Box::new(Ty::Int)),
                    Ty::Str,
                    Ty::Int,
                ])],
            ),
            is_pure: false,
        },
        "udp_recv_bytebuf" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int],
            ret: Ty::Tuple(vec![Ty::Any, Ty::Str, Ty::Int]),
            is_pure: false,
        },
        "udp_recv_bytebuf_timeout" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Int, Ty::Int],
            ret: Ty::App(
                "Option".to_string(),
                vec![Ty::Tuple(vec![Ty::Any, Ty::Str, Ty::Int])],
            ),
            is_pure: false,
        },
        "udp_send_to" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Str, Ty::Int, Ty::Str],
            ret: Ty::Int,
            is_pure: false,
        },
        "udp_send_to_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Str, Ty::Int, Ty::List(Box::new(Ty::Int))],
            ret: Ty::Int,
            is_pure: false,
        },
        "udp_send_bytebuf" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int, Ty::Str, Ty::Int, Ty::Any],
            ret: Ty::Int,
            is_pure: false,
        },
        "udp_close" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Nil,
            is_pure: false,
        },
        "query_where" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::EntityId)),
            is_pure: false,
        },
        "query_map" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::List(Box::new(Ty::Any)),
            is_pure: false,
        },
        "query_count" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Int,
            is_pure: false,
        },
        "with_field" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::List(Box::new(Ty::EntityId)),
                Ty::Str,
                Ty::Str,
                Ty::Fn {
                    params: vec![Ty::Any],
                    ret: Box::new(Ty::Bool),
                    purity: FnPurity::Pure,
                },
            ],
            ret: Ty::List(Box::new(Ty::EntityId)),
            is_pure: false,
        },
        "bitset_new" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::BitSet,
            is_pure: true,
        },
        "bitset_set" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::BitSet, Ty::Int],
            ret: Ty::BitSet,
            is_pure: true,
        },
        "bitset_has" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::BitSet, Ty::Int],
            ret: Ty::Bool,
            is_pure: true,
        },
        "bitset_clear" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::BitSet, Ty::Int],
            ret: Ty::BitSet,
            is_pure: true,
        },
        "buffer_new" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Any,
            is_pure: true,
        },
        "buffer_append" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Str],
            ret: Ty::Any,
            is_pure: true,
        },
        "buffer_to_str" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Str,
            is_pure: true,
        },
        "bytebuf_new" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Int],
            ret: Ty::Any,
            is_pure: true,
        },
        "bytebuf_len" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Int,
            is_pure: true,
        },
        "bytebuf_get" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Int],
            ret: Ty::Int,
            is_pure: true,
        },
        "bytebuf_set_u8" | "bytebuf_set_u32_le" | "bytebuf_set_i32_le" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Int, Ty::Int],
            ret: Ty::Any,
            is_pure: true,
        },
        "bytebuf_get_u32_le" | "bytebuf_get_i32_le" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any, Ty::Int],
            ret: Ty::Int,
            is_pure: true,
        },
        "bytebuf_to_list" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::List(Box::new(Ty::Int)),
            is_pure: true,
        },
        "bytebuf_from_list" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::List(Box::new(Ty::Int))],
            ret: Ty::Any,
            is_pure: true,
        },
        "fork" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::WorldFork,
            is_pure: false,
        },
        // 6th argument (optional): a list of resource values overriding the
        // fork before the rollouts run — candidate seeding without commit()
        // (dogfood feature seq 150 #2). Same validation as `fork_with`.
        "simulate_par" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::WorldFork,
                Ty::List(Box::new(Ty::SystemRef)),
                Ty::Int,
                Ty::Int,
                Ty::Int,
                Ty::List(Box::new(Ty::Any)),
            ],
            ret: Ty::List(Box::new(Ty::WorldFork)),
            is_pure: false,
        },
        // Heterogeneous sibling of simulate_par: a LIST of distinct starting
        // forks, run in parallel under the same schedule (dogfood feature seq 150).
        "simulate_many" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::List(Box::new(Ty::WorldFork)),
                Ty::List(Box::new(Ty::SystemRef)),
                Ty::Int,
                Ty::Int,
            ],
            ret: Ty::List(Box::new(Ty::WorldFork)),
            is_pure: false,
        },
        // One rollout under an EXACT rng seed (no per-index derivation):
        // feed it `fork_seed(f)` of a simulate_par/simulate_many result and
        // it reproduces that single rollout bit-identically.
        "simulate_seeded" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::WorldFork,
                Ty::List(Box::new(Ty::SystemRef)),
                Ty::Int,
                Ty::Int,
            ],
            ret: Ty::WorldFork,
            is_pure: false,
        },
        // Seed a speculative fork without touching the live world: returns a
        // copy of `fork` with one resource overridden.
        "fork_with" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::Any],
            ret: Ty::WorldFork,
            is_pure: false,
        },
        // Which effective rng seed produced this rollout result? 0 for forks
        // that did not come out of the simulate family.
        "fork_seed" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork],
            ret: Ty::Int,
            is_pure: false,
        },
        // 4th argument (optional): data-only input the guest reads back via
        // sandbox_input() — the typed channel that replaces splicing host
        // values into guest source text.
        "sandbox_run" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str, Ty::WorldFork, Ty::Str, Ty::Any],
            ret: Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str]),
            is_pure: false,
        },
        "sandbox_input" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Any,
            is_pure: false,
        },
        "sandbox_output" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Nil,
            is_pure: false,
        },
        "sandbox_last_output" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Any,
            is_pure: false,
        },
        "sandbox_last_fuel" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Int,
            is_pure: false,
        },
        "diff" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::WorldFork],
            ret: Ty::Map(Box::new(Ty::Str), Box::new(Ty::Int)),
            is_pure: true,
        },
        "assert_only_changed" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::WorldFork, Ty::List(Box::new(Ty::Any))],
            ret: Ty::Nil,
            is_pure: true,
        },
        // Causality queries (#4): walk the provenance chain of a value.
        "why" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::EntityId, Ty::Any],
            ret: Ty::Str,
            is_pure: false,
        },
        "why_resource" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Any],
            ret: Ty::Str,
            is_pure: false,
        },
        // Schema migration (#5): serialization is pure, io stays io —
        // compose with write_file/read_file for persistence.
        "save_world" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Str,
            is_pure: false,
        },
        "load_world" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::Int,
            is_pure: false,
        },
        // Fallible sibling: Result<int, str> instead of int-or-abort.
        "try_load_world" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::App("Result".to_string(), vec![Ty::Int, Ty::Str]),
            is_pure: false,
        },
        // Canonical hash of world STATE only (entities, names, components,
        // resources) — no events, provenance, frame counters, or id
        // free-lists. Two machines that converged via sync agree on this
        // even though their fork bytes legitimately differ.
        "world_digest" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Str,
            is_pure: false,
        },
        "schema_digest" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Str,
            is_pure: false,
        },
        // Distributed world merge: the fork wire codec. A fork is full
        // program state (world + in-flight events); these move it between
        // processes. `fork_from_bytes` verifies the embedded digest and runs
        // `migrate` blocks on schema drift, like `load_world`.
        "fork_to_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork],
            ret: Ty::Str,
            is_pure: true,
        },
        "fork_from_bytes" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::Str],
            ret: Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str]),
            is_pure: false,
        },
        // Delta sync: ship only the divergence of `fork` relative to `base`
        // (state *and* provenance); the receiver applies it to its own copy
        // of the same base.
        "fork_delta" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::WorldFork],
            ret: Ty::Str,
            is_pure: true,
        },
        "fork_apply" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::Str],
            ret: Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str]),
            is_pure: false,
        },
        // World merge (#7): three-way merge of forked timelines. Conflicts
        // are data (a list of `Conflict` sum values), not prose.
        "merge_forks" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::WorldFork, Ty::WorldFork],
            ret: Ty::App(
                "Result".to_string(),
                vec![
                    Ty::WorldFork,
                    Ty::List(Box::new(Ty::SumType("Conflict".to_string()))),
                ],
            ),
            is_pure: false,
        },
        // Programmable conflict resolution: re-run the merge with chosen
        // values for specific (conflict, value) pairs. Only field conflicts
        // are resolvable; structural conflicts still refuse.
        "merge_forks_with" => BuiltinSig {
            type_params: vec![],
            params: vec![
                Ty::WorldFork,
                Ty::WorldFork,
                Ty::WorldFork,
                Ty::List(Box::new(Ty::Tuple(vec![
                    Ty::SumType("Conflict".to_string()),
                    Ty::Any,
                ]))),
            ],
            ret: Ty::App(
                "Result".to_string(),
                vec![
                    Ty::WorldFork,
                    Ty::List(Box::new(Ty::SumType("Conflict".to_string()))),
                ],
            ),
            is_pure: false,
        },
        "simulate" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::List(Box::new(Ty::SystemRef)), Ty::Int],
            ret: Ty::WorldFork,
            is_pure: false,
        },
        "commit" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork],
            ret: Ty::Nil,
            is_pure: false,
        },
        "clock" => BuiltinSig {
            type_params: vec![],
            params: vec![],
            ret: Ty::Float,
            is_pure: false,
        },
        "peek" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::EntityId, Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::Any]),
            is_pure: false,
        },
        // the resource dual of peek: read a fork's resource without
        // committing the fork
        "peek_resource" => BuiltinSig {
            type_params: vec![],
            params: vec![Ty::WorldFork, Ty::Any],
            ret: Ty::App("Option".to_string(), vec![Ty::Any]),
            is_pure: false,
        },
        _ => return None,
    };
    Some(sig)
}

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
            _ => None,
        }
    }

    pub fn return_type(self) -> Ty {
        match self {
            Builtin::Print | Builtin::Set | Builtin::SetResource => Ty::Nil,
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
