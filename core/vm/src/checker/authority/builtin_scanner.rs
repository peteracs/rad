use super::*;
use crate::ast::ComponentEntry;
use crate::types::{AuthorityQueryComplexity, AuthorityQueryOperation, Effect, Ty};

impl Scanner<'_> {
    pub(super) fn scan_builtin(&mut self, name: &str, args: &[&Expr]) {
        let declared_effects = builtins::builtin_effect(name);
        self.direct.io |= declared_effects.allows(Effect::IO);
        self.direct.async_effect |= declared_effects.allows(Effect::Async);
        self.record_query_operation(name, args);
        match name {
            "get" | "require" | "require_all" | "has" => self.read_arg(args, 1),
            "read_field" => self.record_direct_field_access(args, false),
            "lookup" | "lookup_all" | "with_field" => self.read_view_or_data_arg(args, 0),
            "entities" => {
                let indexed_view = args.first().is_some_and(|arg| {
                    self.resolver
                        .data_expr(arg, &self.seed.redirects)
                        .is_some_and(|name| self.resolver.known_views.contains_key(&name))
                });
                self.direct.full_scan |= !indexed_view;
                self.direct.allocates = true;
                if args.is_empty() {
                    self.direct.reads.insert(WHOLE_WORLD.to_string());
                } else {
                    for index in 0..args.len() {
                        self.read_view_or_data_arg(args, index);
                    }
                }
            }
            "visit_view" => self.read_view_or_data_arg(args, 0),
            "query_where" | "query_map" => {
                self.direct.full_scan = true;
                self.direct.allocates = true;
                for index in 0..args.len().saturating_sub(1) {
                    self.read_arg(args, index);
                }
                self.record_builtin_callback(args.last());
            }
            "query_count" => {
                self.direct.full_scan = true;
                for index in 0..args.len() {
                    self.read_arg(args, index);
                }
            }
            "res" | "get_resource" | "why_resource" => self.read_arg(args, 0),
            "peek" => self.read_arg(args, 2),
            "why" | "why_field" | "why_removed" => self.read_arg(args, 1),
            "why_missing" => self.read_arg(args, 0),
            "revision"
            | "changes_since"
            | "why_in_view"
            | "why_not_in_view"
            | "why_revision_changed"
            | "why_revision_did_not_change" => self.read_view_or_data_arg(args, 0),
            "lower_bound" | "upper_bound" | "next" | "previous" | "first" | "last" => {
                self.read_arg(args, 0);
            }
            "range" if args.len() == 4 => self.read_arg(args, 0),
            "peek_resource" => self.read_arg(args, 1),
            "set" | "remove" => self.write_arg(args, 1),
            "write_field" => self.record_direct_field_access(args, true),
            "set_resource" => self.write_arg(args, 0),
            "fork_with" => self.write_arg(args, 1),
            "spawn" => self.record_spawn_args(args),
            "despawn" => {
                self.direct.writes.insert(WHOLE_WORLD.to_string());
                self.direct.writes.insert(ENTITY_NAMES.to_string());
                self.direct.writes.insert(ENTITY_IDENTITY.to_string());
            }
            "get_entity" | "require_entity" => {
                self.direct.reads.insert(ENTITY_NAMES.to_string());
            }
            "name_of" => {
                self.direct.reads.insert(ENTITY_IDENTITY.to_string());
            }
            "fork" | "save_world" | "world_digest" => {
                self.direct.reads.insert(WHOLE_WORLD.to_string());
            }
            "schema_digest" => {
                self.direct.reads.insert(SCHEMA.to_string());
            }
            "fork_seed" => {
                self.direct.reads.insert(FORK_STATE.to_string());
            }
            "load_world" | "try_load_world" | "commit" => {
                self.direct.writes.insert(WHOLE_WORLD.to_string());
            }
            "fork_from_bytes" | "fork_apply" | "merge_forks" | "merge_forks_with" => {
                self.direct.writes.insert(WHOLE_WORLD.to_string());
            }
            "simulate" | "simulate_par" | "simulate_many" | "simulate_seeded" => {
                self.record_simulation_schedule(
                    args.get(crate::simulate_syntax::SYSTEMS_ARG_INDEX),
                );
                if name == "simulate_par" {
                    self.record_resource_overrides(args.get(5));
                }
            }
            "sandbox_run" => {
                self.direct.reads.insert(WHOLE_WORLD.to_string());
                self.direct.writes.insert(WHOLE_WORLD.to_string());
                self.direct.unknown = true;
            }
            "recent_events" => {
                self.direct.reads.insert(EVENT_LOG.to_string());
            }
            "emit" => {
                if let Some(event) = args.first().and_then(|arg| {
                    self.resolver
                        .data_expr(arg, &self.seed.redirects)
                        .or_else(|| {
                            if let Expr::ComponentExpr(name, _, _, _) = arg {
                                Some(self.resolver.type_name(name, &self.seed.redirects))
                            } else {
                                None
                            }
                        })
                }) {
                    self.direct.emits.insert(event);
                } else {
                    self.direct.emits.insert(WHOLE_WORLD.to_string());
                }
            }
            "transition" => {
                self.direct.emits.insert(STATE_TRANSITION.to_string());
            }
            "flush_events" => {
                self.direct.emits.insert(EVENT_LOG.to_string());
            }
            "enter_phase" | "mark_phase" | "model_check" => {
                self.direct.writes.insert(LIFECYCLE.to_string());
            }
            "assert_trace" => {
                self.direct.reads.insert(LIFECYCLE.to_string());
            }
            "base_fact" | "candidate_fact" | "why_fact" => self.record_fact(args, false),
            "insert_fact" | "remove_fact" | "replace_fact_by" => self.record_fact(args, true),
            _ => {}
        }
        if matches!(
            name,
            "push"
                | "concat"
                | "map"
                | "filter"
                | "sort"
                | "sort_by"
                | "range"
                | "bytes"
                | "encode_le"
                | "encode_be"
                | "changes_since"
                | "recent_events"
        ) {
            self.direct.allocates = true;
        }
        self.record_typed_builtin_callbacks(name, args);
        assert!(
            builtin_has_exact_state_authority(name)
                || !(declared_effects.allows(Effect::ECS)
                    || declared_effects.allows(Effect::ReadECS)
                    || declared_effects.allows(Effect::Event)),
            "stateful builtin '{name}' has no exact authority metadata"
        );
    }

    fn read_arg(&mut self, args: &[&Expr], index: usize) {
        if let Some(name) = args
            .get(index)
            .and_then(|arg| self.resolver.data_expr(arg, &self.seed.redirects))
        {
            self.direct.reads.insert(name);
        } else {
            self.direct.reads.insert(WHOLE_WORLD.to_string());
        }
    }

    fn record_query_operation(&mut self, name: &str, args: &[&Expr]) {
        let source = |index: usize| {
            args.get(index)
                .and_then(|arg| self.resolver.data_expr(arg, &self.seed.redirects))
                .unwrap_or_else(|| "dynamic".to_string())
        };
        let operation = match name {
            "entities" => {
                let source = if args.is_empty() {
                    WHOLE_WORLD.to_string()
                } else {
                    args.iter()
                        .map(|arg| {
                            self.resolver
                                .data_expr(arg, &self.seed.redirects)
                                .unwrap_or_else(|| "dynamic".to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(" & ")
                };
                let view = args.len() == 1 && self.resolver.known_views.contains_key(&source);
                AuthorityQueryOperation {
                    operation: "entities".to_string(),
                    source,
                    complexity: if view {
                        AuthorityQueryComplexity::OutputLinear
                    } else {
                        AuthorityQueryComplexity::PopulationLinear
                    },
                    allocates: true,
                }
            }
            "visit_view" => AuthorityQueryOperation {
                operation: name.to_string(),
                source: source(0),
                complexity: AuthorityQueryComplexity::OutputLinear,
                allocates: false,
            },
            "lookup" | "get_entity" | "require_entity" | "name_of" => AuthorityQueryOperation {
                operation: name.to_string(),
                source: if matches!(name, "get_entity" | "require_entity") {
                    ENTITY_NAMES.to_string()
                } else if name == "name_of" {
                    ENTITY_IDENTITY.to_string()
                } else {
                    source(0)
                },
                complexity: AuthorityQueryComplexity::Constant,
                allocates: false,
            },
            "lookup_all" => AuthorityQueryOperation {
                operation: name.to_string(),
                source: source(0),
                complexity: AuthorityQueryComplexity::OutputLinear,
                allocates: true,
            },
            "lower_bound" | "upper_bound" | "next" | "previous" | "first" | "last" => {
                AuthorityQueryOperation {
                    operation: name.to_string(),
                    source: source(0),
                    complexity: AuthorityQueryComplexity::Logarithmic,
                    allocates: false,
                }
            }
            "range" if args.len() == 4 => AuthorityQueryOperation {
                operation: name.to_string(),
                source: source(0),
                complexity: AuthorityQueryComplexity::OutputLinear,
                allocates: true,
            },
            "query_where" | "query_map" => AuthorityQueryOperation {
                operation: name.to_string(),
                source: args
                    .iter()
                    .take(args.len().saturating_sub(1))
                    .map(|arg| {
                        self.resolver
                            .data_expr(arg, &self.seed.redirects)
                            .unwrap_or_else(|| "dynamic".to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(" & "),
                complexity: AuthorityQueryComplexity::PopulationLinear,
                allocates: true,
            },
            "query_count" => AuthorityQueryOperation {
                operation: name.to_string(),
                source: args
                    .iter()
                    .map(|arg| {
                        self.resolver
                            .data_expr(arg, &self.seed.redirects)
                            .unwrap_or_else(|| "dynamic".to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(" & "),
                complexity: AuthorityQueryComplexity::PopulationLinear,
                allocates: false,
            },
            "sort" | "sort_by" => AuthorityQueryOperation {
                operation: name.to_string(),
                source: "input collection".to_string(),
                complexity: AuthorityQueryComplexity::PopulationLogLinear,
                allocates: true,
            },
            _ => return,
        };
        self.direct.queries.insert(operation);
    }

    fn record_direct_field_access(&mut self, args: &[&Expr], write: bool) {
        if write {
            self.write_arg(args, 1);
        } else {
            self.read_arg(args, 1);
        }
        let allocation_free = args
            .get(1)
            .and_then(|component| self.resolver.data_expr(component, &self.seed.redirects))
            .zip(args.get(2))
            .and_then(|(component, field)| match field {
                Expr::StrLit(field, _) => Some((component, field)),
                _ => None,
            })
            .is_some_and(|(component, field)| {
                self.resolver
                    .allocation_free_fields
                    .get(&component)
                    .is_some_and(|fields| fields.contains(field))
            });
        self.direct.allocates |= !allocation_free;
    }

    fn read_view_or_data_arg(&mut self, args: &[&Expr], index: usize) {
        let Some(name) = args
            .get(index)
            .and_then(|arg| self.resolver.data_expr(arg, &self.seed.redirects))
        else {
            self.direct.reads.insert(WHOLE_WORLD.to_string());
            return;
        };
        if let Some(dependencies) = self.resolver.known_views.get(&name) {
            self.direct.reads.extend(dependencies.iter().cloned());
        } else {
            self.direct.reads.insert(name);
        }
    }

    fn write_arg(&mut self, args: &[&Expr], index: usize) {
        if let Some(name) = args
            .get(index)
            .and_then(|arg| self.resolver.data_expr(arg, &self.seed.redirects))
        {
            self.direct.writes.insert(name);
        } else {
            self.direct.writes.insert(WHOLE_WORLD.to_string());
        }
    }

    fn record_fact(&mut self, args: &[&Expr], write: bool) {
        let name = match args.first() {
            Some(Expr::StrLit(name, _)) => format!("fact::{name}"),
            _ => "fact::*".to_string(),
        };
        if write {
            self.direct.writes.insert(name);
        } else {
            self.direct.reads.insert(name);
        }
    }

    fn record_builtin_callback(&mut self, callback: Option<&&Expr>) {
        let Some(callback) = callback else {
            self.direct.unknown = true;
            return;
        };
        let argument = self.callable_argument(callback);
        if let Some((index, bound)) = argument.forwarded_param {
            self.dynamic_params.insert(index, bound);
            return;
        }
        let has_targets = !argument.targets.is_empty();
        self.calls.extend(argument.targets.iter().cloned());
        self.plain_calls.extend(argument.targets);
        let has_bound = argument.bound.is_some();
        if let Some(bound) = argument.bound {
            let _ = bound.apply(&mut self.direct);
        }
        if !has_targets && !has_bound {
            self.direct.unknown = true;
        }
    }

    fn record_typed_builtin_callbacks(&mut self, name: &str, args: &[&Expr]) {
        let Some(signature) = builtins::builtin_type_scheme(name) else {
            return;
        };
        for (index, parameter) in signature.params.iter().enumerate() {
            if matches!(parameter, Ty::Fn { .. }) {
                self.record_builtin_callback(args.get(index));
            }
        }
    }

    fn record_simulation_schedule(&mut self, schedule: Option<&&Expr>) {
        let schedule = schedule.copied();
        let items = match schedule {
            Some(Expr::ListLit(items, _)) => Some(items.clone()),
            Some(Expr::Ident(name, _)) => {
                let resolved = self.resolver.symbol_name(name, &self.seed.redirects);
                self.resolver.static_schedules.get(&resolved).cloned()
            }
            _ => None,
        };
        let Some(items) = items else {
            self.direct.unknown = true;
            return;
        };
        for item in items {
            let Expr::SystemRef(path, _) = item else {
                self.direct.unknown = true;
                continue;
            };
            if let Some(target) = self.resolver.system_ref(&path, &self.seed.redirects) {
                self.calls.insert(target.clone());
                self.plain_calls.insert(target);
            } else {
                self.direct.unknown = true;
            }
        }
    }

    fn record_resource_overrides(&mut self, overrides: Option<&&Expr>) {
        let Some(overrides) = overrides.copied() else {
            return;
        };
        let Expr::ListLit(resources, _) = overrides else {
            self.direct.writes.insert(WHOLE_WORLD.to_string());
            self.direct.unknown = true;
            return;
        };
        for resource in resources {
            if let Some(name) = self.resolver.data_expr(resource, &self.seed.redirects) {
                self.direct.writes.insert(name);
            } else {
                self.direct.writes.insert(WHOLE_WORLD.to_string());
                self.direct.unknown = true;
            }
        }
    }

    fn record_spawn_args(&mut self, args: &[&Expr]) {
        self.direct.writes.insert(ENTITIES.to_string());
        self.direct.writes.insert(ENTITY_IDENTITY.to_string());
        if args
            .first()
            .is_some_and(|arg| !matches!(arg, Expr::ComponentExpr(_, _, _, _)))
        {
            self.direct.writes.insert(ENTITY_NAMES.to_string());
        }
        let mut found = false;
        for arg in args {
            if let Some(name) = self.resolver.data_expr(arg, &self.seed.redirects) {
                if matches!(arg, Expr::ComponentExpr(_, _, _, _)) {
                    self.direct.writes.insert(name);
                    found = true;
                }
            }
        }
        if !found {
            self.direct.writes.insert(WHOLE_WORLD.to_string());
        }
    }

    pub(super) fn record_spawn_entries(&mut self, entries: &[ComponentEntry]) {
        self.direct.writes.insert(ENTITIES.to_string());
        self.direct.writes.insert(ENTITY_IDENTITY.to_string());
        let mut found = false;
        for entry in entries {
            match entry {
                ComponentEntry::Init(init) => {
                    let name = self
                        .resolver
                        .type_name(&init.comp_name, &self.seed.redirects);
                    self.direct.writes.insert(name);
                    found = true;
                    for (_, value) in &init.fields {
                        self.scan_expr(value);
                    }
                }
                ComponentEntry::Expr(value) => {
                    if let Some(name) = self.resolver.data_expr(value, &self.seed.redirects) {
                        self.direct.writes.insert(name);
                        found = true;
                    } else {
                        self.direct.writes.insert(WHOLE_WORLD.to_string());
                    }
                    self.scan_expr(value);
                }
            }
        }
        if !found {
            self.direct.writes.insert(WHOLE_WORLD.to_string());
        }
    }
}

fn builtin_has_exact_state_authority(name: &str) -> bool {
    matches!(
        name,
        "get"
            | "read_field"
            | "require"
            | "require_all"
            | "has"
            | "lookup"
            | "lookup_all"
            | "with_field"
            | "entities"
            | "visit_view"
            | "query_where"
            | "query_map"
            | "query_count"
            | "res"
            | "get_resource"
            | "why_resource"
            | "peek"
            | "why"
            | "peek_resource"
            | "set"
            | "write_field"
            | "remove"
            | "set_resource"
            | "fork_with"
            | "spawn"
            | "despawn"
            | "get_entity"
            | "require_entity"
            | "name_of"
            | "fork"
            | "save_world"
            | "world_digest"
            | "schema_digest"
            | "fork_seed"
            | "load_world"
            | "try_load_world"
            | "commit"
            | "fork_from_bytes"
            | "fork_apply"
            | "merge_forks"
            | "merge_forks_with"
            | "simulate"
            | "simulate_par"
            | "simulate_many"
            | "simulate_seeded"
            | "sandbox_run"
            | "recent_events"
            | "emit"
            | "transition"
            | "flush_events"
            | "base_fact"
            | "candidate_fact"
            | "why_fact"
            | "why_field"
            | "why_removed"
            | "why_missing"
            | "why_revision_changed"
            | "why_revision_did_not_change"
            | "revision"
            | "changes_since"
            | "why_in_view"
            | "why_not_in_view"
            | "lower_bound"
            | "upper_bound"
            | "next"
            | "previous"
            | "first"
            | "last"
            | "range"
            | "insert_fact"
            | "remove_fact"
            | "replace_fact_by"
            | "enter_phase"
            | "model_check"
            | "mark_phase"
            | "assert_trace"
    )
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use crate::value::Builtin;

    #[test]
    fn every_stateful_builtin_has_exact_authority_metadata() {
        for builtin in Builtin::ALL {
            let effects = builtins::builtin_effect(builtin.name());
            let stateful = effects.allows(Effect::ECS)
                || effects.allows(Effect::ReadECS)
                || effects.allows(Effect::Event);
            assert!(
                !stateful || builtin_has_exact_state_authority(builtin.name()),
                "stateful builtin '{}' is missing exact authority metadata",
                builtin.name()
            );
        }
        assert!(builtin_has_exact_state_authority("emit"));
    }
}
