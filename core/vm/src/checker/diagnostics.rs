use super::*;
use crate::ast::*;
use crate::types::*;

impl Checker {
    pub(super) fn merge_return_types(&mut self, span: &Span, expected_ret: Option<&Ty>) -> Ty {
        if self.current_fn_returns.is_empty() {
            if let Some(expected) = expected_ret {
                if !expected.assignable_from(&Ty::Nil) && *expected != Ty::Any {
                    self.error(
                        span,
                        format!(
                            "Function declares return type {}, but body returns nil (missing return statement?)",
                            expected
                        ),
                        None,
                    );
                }
                return expected.clone();
            }
            return Ty::Nil;
        }

        if let Some(expected) = expected_ret {
            let mut all_match = true;
            let returns = self.current_fn_returns.clone();
            let is_main = self.current_fn_name.as_deref() == Some("main");
            for (ret_ty, ret_span) in &returns {
                let resolved_ret = self.resolve_ty(ret_ty);
                let expected_resolved = self.resolve_ty(expected);
                let try_propagation_in_main = is_main
                    && (expected_resolved == Ty::Nil || expected_resolved == Ty::Any)
                    && matches!(&resolved_ret,
                        Ty::App(name, _) if name == "Option" || name == "Result"
                    );
                if try_propagation_in_main {
                    continue;
                }
                if !expected_resolved.assignable_from(&resolved_ret)
                    && resolved_ret != Ty::Any
                    && self.subst.unify(&expected_resolved, &resolved_ret).is_err()
                {
                    self.error(
                        ret_span,
                        format!(
                            "Function declares return type {}, but body returns {}",
                            expected_resolved, resolved_ret
                        ),
                        self.type_mismatch_hint(&expected_resolved, &resolved_ret),
                    );
                    all_match = false;
                }
            }
            if all_match {
                return expected.clone();
            } else {
                return Ty::Any;
            }
        }

        let returns = self.current_fn_returns.clone();
        let mut merged = returns[0].0.clone();
        for (ret_ty, _ret_span) in returns.iter().skip(1) {
            merged = Ty::union(&merged, ret_ty);
        }
        merged
    }

    pub(super) fn error(&mut self, span: &Span, message: String, hint: Option<String>) {
        self.errors.push(TypeError {
            line: span.line,
            col: span.col,
            file: span.file,
            message,
            hint,
        });
    }

    pub(super) fn warning(&mut self, span: &Span, message: String, hint: Option<String>) {
        self.warnings.push(TypeWarning {
            line: span.line,
            col: span.col,
            file: span.file,
            message,
            hint,
        });
    }
}

pub(super) fn is_builtin(name: &str) -> bool {
    builtins::is_builtin(name)
}

pub(super) fn is_impure_builtin(name: &str) -> bool {
    matches!(
        name,
        "set"
            | "set_resource"
            | "spawn"
            | "remove"
            | "despawn"
            | "emit"
            | "transition"
            | "flush_events"
            | "input"
            | "readline"
            | "read_file"
            | "write_file"
            | "http_get"
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
            | "udp_close"
            | "now_unix_s"
            | "now_unix_ms"
            | "clock"
            | "load_extension"
            | "rand_int"
            | "rand_float"
            | "rand_bool"
            | "rand_seed"
            | "fork"
            | "simulate"
            | "commit"
            // The speculation/persistence write family. Every one of these
            // mutates or replaces world state, so they are classified impure
            // wherever the classic writers are — the pipeline direct-stage
            // gate consults ONLY this table, and `saved |> load_world` sailed
            // through it while `x |> set` was rejected (dogfood verification,
            // direct-stage probe).
            | "fork_with"
            | "fork_from_bytes"
            | "fork_apply"
            | "simulate_par"
            | "simulate_many"
            | "simulate_seeded"
            | "sandbox_run"
            | "load_world"
            | "try_load_world"
            | "merge_forks"
            | "merge_forks_with"
            // The IO family. Pipelines are specced as pure/readonly
            // computation, but the direct-stage gate consults only this
            // table, so `x |> print` (and a bare `print` as a pipeline
            // callback argument) sailed through while `x |> set` was
            // rejected. Every other consumer of this table already treated
            // these as impure via their BuiltinSigs, so membership here
            // changes pipeline gating only (dogfood seq 254 residual item 3).
            | "print"
            | "eprint"
            | "write_stdout"
            | "write_stderr"
            | "flush_stdout"
            | "read_stdin_all"
            | "append_file"
            | "file_exists"
            | "remove_file"
            | "list_dir"
            | "create_dir"
            | "remove_dir"
            | "read_file_bytes"
            | "write_file_bytes"
            | "sleep_ms"
            | "gc_collect"
            | "log"
            | "metric"
    )
}

pub(super) fn is_readonly_builtin(name: &str) -> bool {
    matches!(
        name,
        "get"
            | "lookup"
            | "lookup_all"
            | "has"
            | "entities"
            | "query_where"
            | "query_map"
            | "query_count"
            | "with_field"
            | "peek"
            | "peek_resource"
            | "res"
            | "get_resource"
            | "name_of"
            | "require"
            | "require_all"
            | "get_entity"
            | "require_entity"
            | "recent_events"
            | "base_fact"
            | "candidate_fact"
    )
}

pub(super) fn builtin_required_effects(name: &str) -> Vec<crate::types::Effect> {
    use crate::types::Effect;
    match name {
        "print" | "eprint" | "write_stdout" | "write_stderr" | "flush_stdout" | "input"
        | "readline" | "read_stdin_all" | "read_file" | "write_file" | "append_file"
        | "file_exists" | "remove_file" | "list_dir" | "create_dir" | "remove_dir"
        | "read_file_bytes" | "write_file_bytes" | "http_get" | "http_post" | "http_post_json"
        | "http_request" | "tcp_connect" | "tcp_listen" | "tcp_accept"
        | "tcp_accept_timeout" | "tcp_read" | "tcp_write" | "tcp_close" | "udp_bind"
        | "udp_recv_from" | "udp_recv_from_timeout" | "udp_recv_from_bytes"
        | "udp_recv_from_bytes_timeout" | "udp_recv_bytebuf" | "udp_recv_bytebuf_timeout"
        | "udp_send_to" | "udp_send_to_bytes" | "udp_send_bytebuf" | "udp_close"
        | "now_unix_s" | "now_unix_ms" | "clock" | "rand_int" | "rand_float"
        | "rand_bool" | "rand_seed" | "load_extension" | "sleep_ms" | "gc_collect"
        | "log" | "metric" => {
            vec![Effect::IO]
        }
        "insert_fact" | "remove_fact" | "replace_fact_by" => vec![Effect::ECS],
        // Every world-mutating builtin must appear here: this table is what
        // keeps effect ANNOTATIONS honest (`readonly fn`/`pure fn` bodies are
        // checked against it), and annotation-trusting consumers — pipeline
        // acceptance, query_where/query_map callbacks — believe a declared
        // effect row. `set_resource` was missing (dogfood verification found
        // a checked query_map mapper mutating a resource through it), as was
        // the whole speculation/persistence write family.
        "set" | "set_resource" | "spawn" | "remove" | "despawn" | "fork" | "fork_with"
        | "simulate" | "simulate_par" | "simulate_many" | "simulate_seeded" | "sandbox_run"
        | "commit" | "load_world" | "try_load_world" | "merge_forks" | "merge_forks_with"
        | "fork_from_bytes" | "fork_apply" => {
            vec![Effect::ECS]
        }
        "get" | "lookup" | "lookup_all" | "has" | "entities" | "query_where" | "query_map"
        | "query_count" | "with_field" | "peek" | "peek_resource" | "res" | "name_of"
        | "require" | "require_all" | "get_entity" | "require_entity" | "recent_events"
        | "get_resource"
        // The diagnostic/persistence READ family: world reads every one, so a
        // `pure fn` must not be able to hide them (they were classified
        // ReadECS in builtin_effect but absent here, leaving the annotation
        // boundary blind to them — dogfood table-parity audit, seq 254
        // residual list item 2).
        | "save_world" | "world_digest" | "schema_digest" | "why" | "why_resource"
        | "why_fact"
        | "fork_seed" => {
            vec![Effect::ReadECS]
        }
        "emit" | "transition" | "flush_events" => vec![Effect::Event],
        _ => vec![],
    }
}

pub(super) fn is_immutable_transform_builtin(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "pop_last"
            | "drop_last"
            | "sort"
            | "sort_by"
            | "reverse"
            | "slice"
            | "map"
            | "filter"
            | "append"
            | "extend"
            | "flat_map"
            | "zip"
            | "split"
            | "trim"
            | "replace"
            | "bitset_set"
            | "bitset_clear"
            | "buffer_append"
            | "bytebuf_set_u8"
            | "bytebuf_set_u32_le"
            | "bytebuf_set_i32_le"
    )
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// If `name` is close to one of `candidates` (edit distance), return a short hint.
pub(super) fn suggest_did_you_mean(name: &str, candidates: &[&str]) -> Option<String> {
    if name.is_empty() || candidates.is_empty() {
        return None;
    }
    let mut best: Option<(&str, usize)> = None;
    for c in candidates {
        if c.is_empty() || *c == name {
            continue;
        }
        let d = levenshtein(name, c);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((*c, d));
        }
    }
    let (cand, dist) = best?;
    let max_ok = if name.chars().count() <= 5 { 2 } else { 3 };
    if dist <= max_ok {
        Some(format!("Did you mean '{}'?", cand))
    } else {
        None
    }
}

/// Suggest builtin conversions when a value's type does not match the expected type.
pub(super) fn suggest_type_fix(expected: &str, actual: &str) -> Option<String> {
    if expected == actual || expected == "any" || actual == "any" {
        return None;
    }
    match expected {
        "str" if actual != "str" && actual != "nil" => Some(format!(
            "Try str(...) to convert this {} value to str",
            actual
        )),
        "int" if actual == "float" => Some("Try int(...) to convert from float".to_string()),
        "int" if actual == "str" => {
            Some("Try int(...) if the string contains an integer".to_string())
        }
        "float" if actual == "int" => {
            Some("Try float(...) if you need an explicit float".to_string())
        }
        "float" if actual == "str" => {
            Some("Try float(...) if the string contains a number".to_string())
        }
        _ => None,
    }
}

pub(super) fn ignored_immutable_transform_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Call(callee, _, _) => {
            if let Expr::Ident(name, _) = callee.as_ref() {
                if is_immutable_transform_builtin(name) {
                    return Some(name.as_str());
                }
            }
            None
        }
        Expr::Pipe(_, right, _) => ignored_immutable_transform_name(right),
        Expr::Ident(name, _) => {
            if is_immutable_transform_builtin(name) {
                Some(name.as_str())
            } else {
                None
            }
        }
        _ => None,
    }
}
