use super::*;

impl Checker {
    pub(crate) fn infer_and_enforce_authority(&mut self, program: &Program) {
        let mut seeds = Vec::<Seed>::new();
        let mut handlers = BTreeMap::<String, Vec<String>>::new();
        let mut static_schedules = HashMap::<String, Vec<Expr>>::new();
        let mut phases = HashMap::<String, Vec<String>>::new();
        collect_static_schedules(
            &program.declarations,
            &HashMap::new(),
            &mut static_schedules,
        );
        collect_phases(&program.declarations, &HashMap::new(), &mut phases);
        collect_seeds(
            &program.declarations,
            None,
            &HashMap::new(),
            &self.type_redirects,
            &mut seeds,
            &mut handlers,
        );
        for (alias, declarations) in &self.alias_decls {
            let redirects = alias_redirects(alias, declarations);
            collect_static_schedules(declarations, &redirects, &mut static_schedules);
            collect_phases(declarations, &redirects, &mut phases);
            collect_seeds(
                declarations,
                Some(alias),
                &redirects,
                &self.type_redirects,
                &mut seeds,
                &mut handlers,
            );
        }

        let known_callables = seeds.iter().map(|seed| seed.name.clone()).collect();
        let known_data = self
            .components
            .keys()
            .chain(self.resources.keys())
            .cloned()
            .collect();
        let resolver = Resolver {
            known_callables,
            known_data,
            module_aliases: self.module_aliases.clone(),
            type_redirects: self.type_redirects.clone(),
            static_schedules,
            phases,
        };

        let mut nodes = BTreeMap::<String, NodeDraft>::new();
        let mut call_sites = Vec::new();
        let mut pending = VecDeque::from(seeds);
        while let Some(seed) = pending.pop_front() {
            if nodes.contains_key(&seed.name) {
                continue;
            }
            let (node, closures, sites) = Scanner::new(&resolver, &seed).scan();
            pending.extend(closures);
            call_sites.extend(sites);
            nodes.insert(seed.name.clone(), node);
        }

        connect_forwarded_parameter_requirements(&mut nodes, &call_sites);
        connect_handlers(&mut nodes, &handlers);
        let base_nodes = nodes;
        let mut union_nodes = base_nodes.clone();
        connect_callbacks(&mut union_nodes, call_sites.clone());
        let union_synchronous = freeze_graph(&union_nodes, false);
        let mut report = freeze_graph(&union_nodes, true);
        for (name, callable) in &mut report.callables {
            if let Some(synchronous) = union_synchronous.callables.get(name) {
                callable.synchronous = synchronous.transitive.clone();
            }
        }

        for node in base_nodes.values() {
            if node.seed.kind != AuthorityCallableKind::System {
                continue;
            }
            let specialized = specialize_system_graph(&base_nodes, &call_sites, &node.seed.name);
            let synchronous = freeze_graph(&specialized, false);
            self.enforce_system_authority(node, &synchronous);
            let full = freeze_graph(&specialized, true);
            for (name, mut callable) in full.callables {
                if let Some(sync) = synchronous.callables.get(&name) {
                    callable.synchronous = sync.transitive.clone();
                }
                report.callables.insert(name, callable);
            }
        }
        rebuild_reverse_indexes(&mut report);
        self.authority = report;
    }

    fn enforce_system_authority(&mut self, node: &NodeDraft, report: &AuthorityReport) {
        let Some(inferred) = report.callables.get(&node.seed.name) else {
            return;
        };
        let mut allowed_reads = node
            .seed
            .system_params
            .values()
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>();
        allowed_reads.extend(node.seed.authority_reads.iter().map(String::as_str));
        let mut allowed_writes = node
            .seed
            .system_params
            .values()
            .filter(|(_, mutable)| *mutable)
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>();
        allowed_writes.extend(node.seed.authority_writes.iter().map(String::as_str));
        let allowed_emits = node
            .seed
            .authority_emits
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let reads_all = allowed_reads.contains(WHOLE_WORLD);
        let writes_all = allowed_writes.contains(WHOLE_WORLD);
        let emits_all = allowed_emits.contains(WHOLE_WORLD);
        let denied_reads = inferred
            .transitive
            .reads
            .iter()
            .filter(|name| !reads_all && !allowed_reads.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let denied_writes = inferred
            .transitive
            .writes
            .iter()
            .filter(|name| {
                !writes_all
                    && !allowed_writes.contains(name.as_str())
                    && !node.parameter_assignments.contains(name.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        let denied_emits = inferred
            .transitive
            .emits
            .iter()
            .filter(|name| !emits_all && !allowed_emits.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let denied_io = inferred.transitive.io && !node.seed.authority_io;
        let denied_async = inferred.transitive.async_effect && !node.seed.authority_async;
        if denied_reads.is_empty()
            && denied_writes.is_empty()
            && denied_emits.is_empty()
            && !denied_io
            && !denied_async
            && !inferred.transitive.unknown
        {
            return;
        }
        let mut violations = Vec::new();
        if !denied_reads.is_empty() {
            violations.push(format!("reads [{}]", denied_reads.join(", ")));
        }
        if !denied_writes.is_empty() {
            violations.push(format!("writes [{}]", denied_writes.join(", ")));
        }
        if !denied_emits.is_empty() {
            violations.push(format!("emits [{}]", denied_emits.join(", ")));
        }
        if denied_io {
            violations.push("performs IO".to_string());
        }
        if denied_async {
            violations.push("reaches async execution".to_string());
        }
        if inferred.transitive.unknown {
            violations.push("invokes an unbounded function value".to_string());
        }
        let evidence = denied_writes
            .first()
            .map(|name| (name.as_str(), true))
            .or_else(|| denied_reads.first().map(|name| (name.as_str(), false)))
            .and_then(|(name, write)| effect_path(report, &node.seed.name, name, write))
            .or_else(|| {
                denied_emits.first().and_then(|name| {
                    effect_path_matching(report, &node.seed.name, |effects| {
                        effects.emits.iter().any(|event| event == name)
                    })
                })
            })
            .or_else(|| {
                denied_io
                    .then(|| effect_path_matching(report, &node.seed.name, |effects| effects.io))?
            })
            .or_else(|| {
                denied_async.then(|| {
                    effect_path_matching(report, &node.seed.name, |effects| effects.async_effect)
                })?
            })
            .or_else(|| {
                inferred
                    .transitive
                    .unknown
                    .then(|| unknown_effect_path(report, &node.seed.name))
                    .flatten()
            })
            .expect("every transitive authority violation must have a direct call path");
        let mut grants = Vec::new();
        grants.extend(denied_reads.iter().map(|name| format!("reads {name}")));
        grants.extend(denied_writes.iter().map(|name| format!("writes {name}")));
        grants.extend(denied_emits.iter().map(|name| {
            if name.starts_with('$') {
                format!("emits \"{name}\"")
            } else {
                format!("emits {name}")
            }
        }));
        if denied_io {
            grants.push("io true".to_string());
        }
        if denied_async {
            grants.push("async true".to_string());
        }
        self.authority_error(
                &node.seed.span,
                format!(
                    "System '{}' exceeds its declared authority: {}",
                    node.seed.display_name,
                    violations.join("; ")
                ),
                Some(format!(
                    "authority path: {}. Add `{}` to the system signature, or move the effect behind a separately scheduled authority boundary",
                    evidence.join(" -> "),
                    grants.join("`, `")
                )),
            );
    }
}

fn collect_seeds(
    declarations: &[Decl],
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
    type_redirects: &HashMap<String, String>,
    seeds: &mut Vec<Seed>,
    handlers: &mut BTreeMap<String, Vec<String>>,
) {
    let mut handler_counts = HashMap::<String, usize>::new();
    for declaration in declarations {
        match declaration {
            Decl::Fn(function) => seeds.push(fn_seed(function, alias, redirects)),
            Decl::System(system) => {
                seeds.push(system_seed(system, alias, redirects, type_redirects))
            }
            Decl::OnHandler(handler) => {
                let event = canonical_decl_name(&handler.event_name, redirects, type_redirects);
                let count = handler_counts.entry(event.clone()).or_default();
                *count += 1;
                let seed = handler_seed(handler, alias, redirects, &event, *count);
                handlers.entry(event).or_default().push(seed.name.clone());
                seeds.push(seed);
            }
            Decl::Stmt(Stmt::Let(binding)) if binding.names.len() == 1 => {
                if matches!(&binding.value, Expr::FnExpr(..)) {
                    seeds.push(closure_binding_seed(binding, alias, redirects));
                } else if let Some(bound) = binding
                    .type_annotation
                    .as_ref()
                    .and_then(CallableBound::from_type)
                {
                    seeds.push(typed_binding_seed(binding, alias, redirects, bound));
                }
            }
            _ => {}
        }
    }
}

fn collect_static_schedules(
    declarations: &[Decl],
    redirects: &HashMap<String, String>,
    schedules: &mut HashMap<String, Vec<Expr>>,
) {
    for declaration in declarations {
        let Decl::Stmt(Stmt::Let(binding)) = declaration else {
            continue;
        };
        if binding.mutable || binding.tuple_destructure || binding.names.len() != 1 {
            continue;
        }
        let crate::simulate_syntax::SystemsListForm::StaticSchedule(items) =
            crate::simulate_syntax::classify_systems_argument(&binding.value)
        else {
            continue;
        };
        let name = redirects
            .get(&binding.names[0])
            .cloned()
            .unwrap_or_else(|| binding.names[0].clone());
        schedules.insert(name, items.to_vec());
    }
}

fn collect_phases(
    declarations: &[Decl],
    redirects: &HashMap<String, String>,
    phases: &mut HashMap<String, Vec<String>>,
) {
    for declaration in declarations {
        let Decl::Phase(phase) = declaration else {
            continue;
        };
        let name = redirects
            .get(&phase.name)
            .cloned()
            .unwrap_or_else(|| phase.name.clone());
        let systems = phase
            .systems
            .iter()
            .map(|system| {
                redirects
                    .get(system)
                    .cloned()
                    .unwrap_or_else(|| system.clone())
            })
            .collect();
        phases.insert(name, systems);
    }
}

fn fn_seed(function: &FnDecl, alias: Option<&str>, redirects: &HashMap<String, String>) -> Seed {
    let name = redirects
        .get(&function.name)
        .cloned()
        .unwrap_or_else(|| function.name.clone());
    let display_name = alias.map_or_else(
        || function.name.clone(),
        |alias| format!("{alias}.{}", function.name),
    );
    let declared = EffectDraft {
        async_effect: function.is_async,
        ..EffectDraft::default()
    };
    Seed {
        name,
        display_name,
        kind: AuthorityCallableKind::Function,
        block: function.body.clone(),
        span: function.span.clone(),
        redirects: redirects.clone(),
        params: function.params.clone(),
        param_bounds: callable_param_bounds(&function.param_types),
        system_params: BTreeMap::new(),
        authority_reads: BTreeSet::new(),
        authority_writes: BTreeSet::new(),
        authority_emits: BTreeSet::new(),
        authority_io: false,
        authority_async: false,
        declared,
        captured_locals: HashMap::new(),
    }
}

fn system_seed(
    system: &SystemDecl,
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
    type_redirects: &HashMap<String, String>,
) -> Seed {
    let name = redirects
        .get(&system.name)
        .cloned()
        .unwrap_or_else(|| system.name.clone());
    let display_name = alias.map_or_else(
        || system.name.clone(),
        |alias| format!("{alias}.{}", system.name),
    );
    let system_params = system
        .params
        .iter()
        .map(|(param, mutable, data)| {
            (
                param.clone(),
                (
                    canonical_decl_name(data, redirects, type_redirects),
                    *mutable,
                ),
            )
        })
        .collect();
    Seed {
        name,
        display_name,
        kind: AuthorityCallableKind::System,
        block: system.body.clone(),
        span: system.span.clone(),
        redirects: redirects.clone(),
        params: Vec::new(),
        param_bounds: BTreeMap::new(),
        system_params,
        authority_reads: system
            .authority_reads
            .iter()
            .map(|authority| canonical_decl_name(authority, redirects, type_redirects))
            .collect(),
        authority_writes: system
            .authority_writes
            .iter()
            .map(|authority| canonical_decl_name(authority, redirects, type_redirects))
            .collect(),
        authority_emits: system
            .authority_emits
            .iter()
            .map(|authority| canonical_decl_name(authority, redirects, type_redirects))
            .collect(),
        authority_io: system.authority_io,
        authority_async: system.authority_async,
        declared: EffectDraft::default(),
        captured_locals: HashMap::new(),
    }
}

fn handler_seed(
    handler: &OnHandler,
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
    event: &str,
    count: usize,
) -> Seed {
    let prefix = alias.map_or_else(String::new, |alias| format!("{alias}."));
    let file = handler.span.file.map_or(0, |file| file.0);
    let declared = EffectDraft {
        async_effect: handler.is_async,
        ..EffectDraft::default()
    };
    Seed {
        name: format!(
            "@handler:{prefix}{event}:{file}:{}:{}",
            handler.span.line, handler.span.col
        ),
        display_name: format!("on {prefix}{event}#{count}"),
        kind: AuthorityCallableKind::Handler,
        block: handler.body.clone(),
        span: handler.span.clone(),
        redirects: redirects.clone(),
        params: vec![handler.param_name.clone()],
        param_bounds: BTreeMap::new(),
        system_params: BTreeMap::new(),
        authority_reads: BTreeSet::new(),
        authority_writes: BTreeSet::new(),
        authority_emits: BTreeSet::new(),
        authority_io: false,
        authority_async: false,
        declared,
        captured_locals: HashMap::new(),
    }
}

fn closure_binding_seed(
    binding: &crate::ast::LetStmt,
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
) -> Seed {
    let Expr::FnExpr(params, _, param_types, _, _, body, span) = &binding.value else {
        unreachable!("top-level closure seed requires FnExpr")
    };
    let local_name = &binding.names[0];
    let name = redirects
        .get(local_name)
        .cloned()
        .unwrap_or_else(|| local_name.clone());
    let display_name = alias.map_or_else(
        || local_name.clone(),
        |alias| format!("{alias}.{local_name}"),
    );
    Seed {
        name,
        display_name,
        kind: AuthorityCallableKind::Closure,
        block: body.clone(),
        span: span.clone(),
        redirects: redirects.clone(),
        params: params.clone(),
        param_bounds: callable_param_bounds(param_types),
        system_params: BTreeMap::new(),
        authority_reads: BTreeSet::new(),
        authority_writes: BTreeSet::new(),
        authority_emits: BTreeSet::new(),
        authority_io: false,
        authority_async: false,
        declared: EffectDraft::default(),
        captured_locals: HashMap::new(),
    }
}

fn typed_binding_seed(
    binding: &crate::ast::LetStmt,
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
    bound: CallableBound,
) -> Seed {
    let local_name = &binding.names[0];
    let name = redirects
        .get(local_name)
        .cloned()
        .unwrap_or_else(|| local_name.clone());
    let display_name = alias.map_or_else(
        || local_name.clone(),
        |alias| format!("{alias}.{local_name}"),
    );
    let mut declared = EffectDraft::default();
    bound.apply(&mut declared);
    Seed {
        name,
        display_name,
        kind: AuthorityCallableKind::Closure,
        block: Block {
            id: binding.id,
            span: binding.span.clone(),
            stmts: Vec::new(),
        },
        span: binding.span.clone(),
        redirects: redirects.clone(),
        params: Vec::new(),
        param_bounds: BTreeMap::new(),
        system_params: BTreeMap::new(),
        authority_reads: BTreeSet::new(),
        authority_writes: BTreeSet::new(),
        authority_emits: BTreeSet::new(),
        authority_io: false,
        authority_async: false,
        declared,
        captured_locals: HashMap::new(),
    }
}

fn callable_param_bounds(annotations: &[Option<TypeExpr>]) -> BTreeMap<usize, CallableBound> {
    annotations
        .iter()
        .enumerate()
        .filter_map(|(index, annotation)| {
            annotation
                .as_ref()
                .and_then(CallableBound::from_type)
                .map(|bound| (index, bound))
        })
        .collect()
}

fn alias_redirects(alias: &str, declarations: &[Decl]) -> HashMap<String, String> {
    let mut redirects = HashMap::new();
    for declaration in declarations {
        if let Some(name) = authority_decl_name(declaration) {
            redirects.insert(name.to_string(), format!("__mod_{alias}__{name}"));
        } else if let Decl::Stmt(Stmt::Let(binding)) = declaration {
            for name in &binding.names {
                redirects.insert(name.clone(), format!("__mod_{alias}__{name}"));
            }
        }
    }
    redirects
}

fn authority_decl_name(declaration: &Decl) -> Option<&str> {
    match declaration {
        Decl::Component(item) | Decl::Struct(item) => Some(&item.name),
        Decl::Resource(item) => Some(&item.name),
        Decl::State(item) => Some(&item.name),
        Decl::System(item) => Some(&item.name),
        Decl::Event(item) => Some(&item.name),
        Decl::Phase(item) => Some(&item.name),
        Decl::Fn(item) => Some(&item.name),
        Decl::Type(item) => Some(&item.name),
        Decl::TypeAlias(item) => Some(&item.name),
        _ => None,
    }
}

fn canonical_decl_name(
    raw: &str,
    redirects: &HashMap<String, String>,
    type_redirects: &HashMap<String, String>,
) -> String {
    let mut current = redirects
        .get(raw)
        .cloned()
        .unwrap_or_else(|| raw.to_string());
    let mut seen = HashSet::new();
    while seen.insert(current.clone()) {
        let Some(next) = type_redirects.get(&current) else {
            break;
        };
        current = next.clone();
    }
    current
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
struct ResolvedCallback {
    targets: BTreeSet<String>,
    bound: Option<CallableBound>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SpecializedState {
    callable: String,
    callbacks: BTreeMap<usize, ResolvedCallback>,
}

fn specialize_system_graph(
    nodes: &BTreeMap<String, NodeDraft>,
    call_sites: &[CallSite],
    root: &str,
) -> BTreeMap<String, NodeDraft> {
    let mut sites_by_caller = BTreeMap::<String, Vec<&CallSite>>::new();
    for site in call_sites {
        sites_by_caller
            .entry(site.caller.clone())
            .or_default()
            .push(site);
    }

    let root_state = SpecializedState {
        callable: root.to_string(),
        callbacks: BTreeMap::new(),
    };
    let mut pending = VecDeque::from([root_state.clone()]);
    let mut discovered = BTreeSet::from([root_state]);
    let mut specialized = BTreeMap::new();

    while let Some(state) = pending.pop_front() {
        let Some(base) = nodes.get(&state.callable) else {
            continue;
        };
        let key = specialized_state_name(root, &state);
        let mut node = base.clone();
        node.seed.name = key.clone();
        node.calls.clear();
        node.plain_calls.clear();
        node.deferred_calls.clear();

        let mut synchronous_children = BTreeSet::new();
        for (&index, &declared_bound) in &base.dynamic_params {
            let resolution =
                state
                    .callbacks
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| ResolvedCallback {
                        targets: BTreeSet::new(),
                        bound: Some(declared_bound),
                    });
            if let Some(bound) = resolution.bound {
                let _ = bound.apply(&mut node.direct);
            }
            for target in resolution.targets {
                if nodes.contains_key(&target) {
                    synchronous_children.insert(SpecializedState {
                        callable: target,
                        callbacks: BTreeMap::new(),
                    });
                }
            }
        }

        if let Some(sites) = sites_by_caller.get(&state.callable) {
            for site in sites {
                let Some(callee) = nodes.get(&site.callee) else {
                    continue;
                };
                let mut callbacks = BTreeMap::new();
                for (&index, &declared_bound) in &callee.dynamic_params {
                    let argument = site.args.get(index).cloned().unwrap_or_default();
                    let mut resolved =
                        if let Some((forwarded, forwarded_bound)) = argument.forwarded_param {
                            state.callbacks.get(&forwarded).cloned().unwrap_or_else(|| {
                                ResolvedCallback {
                                    targets: BTreeSet::new(),
                                    bound: Some(forwarded_bound),
                                }
                            })
                        } else {
                            ResolvedCallback {
                                targets: argument.targets,
                                bound: argument.bound,
                            }
                        };
                    if resolved.targets.is_empty() && resolved.bound.is_none() {
                        resolved.bound = Some(declared_bound);
                    }
                    callbacks.insert(index, resolved);
                }
                synchronous_children.insert(SpecializedState {
                    callable: site.callee.clone(),
                    callbacks,
                });
            }
        }
        for target in &base.plain_calls {
            if nodes.contains_key(target) {
                synchronous_children.insert(SpecializedState {
                    callable: target.clone(),
                    callbacks: BTreeMap::new(),
                });
            }
        }

        for child in synchronous_children {
            let child_name = specialized_state_name(root, &child);
            node.calls.insert(child_name);
            if discovered.insert(child.clone()) {
                pending.push_back(child);
            }
        }
        for target in &base.deferred_calls {
            if !nodes.contains_key(target) {
                continue;
            }
            let child = SpecializedState {
                callable: target.clone(),
                callbacks: BTreeMap::new(),
            };
            let child_name = specialized_state_name(root, &child);
            node.deferred_calls.insert(child_name);
            if discovered.insert(child.clone()) {
                pending.push_back(child);
            }
        }
        specialized.insert(key, node);
    }
    specialized
}

fn specialized_state_name(root: &str, state: &SpecializedState) -> String {
    if state.callable == root && state.callbacks.is_empty() {
        return root.to_string();
    }
    fn segment(output: &mut String, value: &str) {
        output.push_str(&value.len().to_string());
        output.push(':');
        output.push_str(value);
    }
    let mut output = "@specialized:".to_string();
    segment(&mut output, root);
    segment(&mut output, &state.callable);
    for (index, callback) in &state.callbacks {
        output.push('|');
        output.push_str(&index.to_string());
        output.push(':');
        output.push(match callback.bound {
            None => '-',
            Some(CallableBound::Pure) => 'p',
            Some(CallableBound::Readonly) => 'r',
            Some(CallableBound::Unbounded) => 'u',
        });
        output.push(':');
        output.push_str(&callback.targets.len().to_string());
        for target in &callback.targets {
            output.push(':');
            segment(&mut output, target);
        }
    }
    output
}

fn connect_forwarded_parameter_requirements(
    nodes: &mut BTreeMap<String, NodeDraft>,
    call_sites: &[CallSite],
) {
    let mut changed = true;
    while changed {
        changed = false;
        for site in call_sites {
            let dynamic = nodes
                .get(&site.callee)
                .map(|node| node.dynamic_params.clone())
                .unwrap_or_default();
            for index in dynamic.keys() {
                let Some((forwarded, bound)) = site
                    .args
                    .get(*index)
                    .and_then(|argument| argument.forwarded_param)
                else {
                    continue;
                };
                if let Some(caller) = nodes.get_mut(&site.caller) {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        caller.dynamic_params.entry(forwarded)
                    {
                        entry.insert(bound);
                        changed = true;
                    }
                }
            }
        }
    }
}

fn connect_callbacks(nodes: &mut BTreeMap<String, NodeDraft>, call_sites: Vec<CallSite>) {
    let mut resolutions = BTreeMap::<(String, usize), CallableArgument>::new();
    let mut changed = true;
    while changed {
        changed = false;
        for site in &call_sites {
            let dynamic = nodes
                .get(&site.callee)
                .map(|node| node.dynamic_params.clone())
                .unwrap_or_default();
            for (index, callee_bound) in dynamic {
                let mut contribution = site.args.get(index).cloned().unwrap_or_default();
                let was_forwarded = contribution.forwarded_param.is_some();
                if let Some((forwarded_index, forwarded_bound)) = contribution.forwarded_param {
                    if let Some(caller) = nodes.get_mut(&site.caller) {
                        match caller.dynamic_params.entry(forwarded_index) {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(forwarded_bound);
                                changed = true;
                            }
                            std::collections::btree_map::Entry::Occupied(_) => {}
                        }
                    }
                    contribution = resolutions
                        .get(&(site.caller.clone(), forwarded_index))
                        .cloned()
                        .unwrap_or_default();
                }
                if contribution.targets.is_empty() && contribution.bound.is_none() && !was_forwarded
                {
                    contribution.bound = Some(callee_bound);
                }
                contribution.forwarded_param = None;
                changed |= resolutions
                    .entry((site.callee.clone(), index))
                    .or_default()
                    .merge(&contribution);
            }
        }

        let observed = call_sites
            .iter()
            .flat_map(|site| {
                nodes.get(&site.callee).into_iter().flat_map(|callee| {
                    callee
                        .dynamic_params
                        .keys()
                        .map(|index| (site.callee.clone(), *index))
                })
            })
            .collect::<BTreeSet<_>>();
        for node in nodes.values() {
            for (&index, &bound) in &node.dynamic_params {
                if !observed.contains(&(node.seed.name.clone(), index)) {
                    changed |= resolutions
                        .entry((node.seed.name.clone(), index))
                        .or_default()
                        .merge(&CallableArgument {
                            targets: BTreeSet::new(),
                            forwarded_param: None,
                            bound: Some(bound),
                        });
                }
            }
        }
    }

    for ((callable, _), resolution) in resolutions {
        let known_targets = resolution
            .targets
            .into_iter()
            .filter(|target| nodes.contains_key(target))
            .collect::<Vec<_>>();
        if let Some(node) = nodes.get_mut(&callable) {
            node.calls.extend(known_targets.iter().cloned());
            node.plain_calls.extend(known_targets);
            if let Some(bound) = resolution.bound {
                let _ = bound.apply(&mut node.direct);
            }
        }
    }
}

fn connect_handlers(
    nodes: &mut BTreeMap<String, NodeDraft>,
    handlers: &BTreeMap<String, Vec<String>>,
) {
    for node in nodes.values_mut() {
        let emitted = node.direct.emits.clone();
        for event in emitted {
            if event == WHOLE_WORLD || event == EVENT_LOG {
                let targets = handlers
                    .values()
                    .flat_map(|targets| targets.iter().cloned());
                if event == EVENT_LOG {
                    let targets = targets.collect::<Vec<_>>();
                    node.calls.extend(targets.iter().cloned());
                    node.plain_calls.extend(targets);
                } else {
                    node.deferred_calls.extend(targets);
                }
            } else if let Some(targets) = handlers.get(&event) {
                node.deferred_calls.extend(targets.iter().cloned());
            }
        }
    }
}

fn freeze_graph(nodes: &BTreeMap<String, NodeDraft>, include_deferred: bool) -> AuthorityReport {
    let keys = nodes.keys().cloned().collect::<Vec<_>>();
    let indexes = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut effects = keys
        .iter()
        .map(|key| nodes[key].direct.clone())
        .collect::<Vec<_>>();
    let mut callers = vec![Vec::<usize>::new(); keys.len()];
    for (caller_index, key) in keys.iter().enumerate() {
        let callees = nodes[key].calls.iter().chain(
            include_deferred
                .then_some(&nodes[key].deferred_calls)
                .into_iter()
                .flatten(),
        );
        for callee in callees {
            if let Some(&callee_index) = indexes.get(callee) {
                callers[callee_index].push(caller_index);
            }
        }
    }
    let mut queue = VecDeque::from_iter(0..keys.len());
    let mut queued = vec![true; keys.len()];
    while let Some(callee) = queue.pop_front() {
        queued[callee] = false;
        let inherited = effects[callee].clone();
        for &caller in &callers[callee] {
            if effects[caller].merge(&inherited) && !queued[caller] {
                queued[caller] = true;
                queue.push_back(caller);
            }
        }
    }

    let mut report = AuthorityReport::default();
    for (index, key) in keys.iter().enumerate() {
        let node = &nodes[key];
        let calls = node
            .calls
            .iter()
            .chain(
                include_deferred
                    .then_some(&node.deferred_calls)
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect::<BTreeSet<_>>();
        let callable = CallableAuthority {
            name: key.clone(),
            display_name: node.seed.display_name.clone(),
            kind: node.seed.kind,
            direct: node.direct.freeze(),
            synchronous: effects[index].freeze(),
            transitive: effects[index].freeze(),
            calls: calls.into_iter().collect(),
            deferred_calls: node.deferred_calls.iter().cloned().collect(),
            line: node.seed.span.line,
            col: node.seed.span.col,
        };
        for read in &callable.transitive.reads {
            report
                .readers
                .entry(read.clone())
                .or_default()
                .push(callable.display_name.clone());
        }
        for write in &callable.transitive.writes {
            report
                .writers
                .entry(write.clone())
                .or_default()
                .push(callable.display_name.clone());
        }
        report.callables.insert(key.clone(), callable);
    }
    for names in report
        .readers
        .values_mut()
        .chain(report.writers.values_mut())
    {
        names.sort();
        names.dedup();
    }
    report
}

fn rebuild_reverse_indexes(report: &mut AuthorityReport) {
    report.readers.clear();
    report.writers.clear();
    for (key, callable) in &report.callables {
        // Specialized helper nodes exist to retain exact system call paths.
        // Their unspecialized callable already represents the helper in the
        // reverse indexes; including both would duplicate user-facing names.
        if key.starts_with("@specialized:") {
            continue;
        }
        for read in &callable.transitive.reads {
            report
                .readers
                .entry(read.clone())
                .or_default()
                .push(callable.display_name.clone());
        }
        for write in &callable.transitive.writes {
            report
                .writers
                .entry(write.clone())
                .or_default()
                .push(callable.display_name.clone());
        }
    }
    for names in report
        .readers
        .values_mut()
        .chain(report.writers.values_mut())
    {
        names.sort();
        names.dedup();
    }
}

fn effect_path(
    report: &AuthorityReport,
    start: &str,
    authority: &str,
    write: bool,
) -> Option<Vec<String>> {
    effect_path_matching(report, start, |effects| {
        let names = if write {
            &effects.writes
        } else {
            &effects.reads
        };
        names.iter().any(|name| name == authority)
    })
}

fn unknown_effect_path(report: &AuthorityReport, start: &str) -> Option<Vec<String>> {
    effect_path_matching(report, start, |effects| effects.unknown)
}

fn effect_path_matching(
    report: &AuthorityReport,
    start: &str,
    matches: impl Fn(&AuthorityEffects) -> bool,
) -> Option<Vec<String>> {
    let mut queue = VecDeque::from([start.to_string()]);
    let mut previous = HashMap::<String, String>::from([(start.to_string(), String::new())]);
    while let Some(current) = queue.pop_front() {
        let node = report.callables.get(&current)?;
        if matches(&node.direct) {
            let mut keys = vec![current.clone()];
            let mut cursor = current;
            while let Some(parent) = previous.get(&cursor) {
                if parent.is_empty() {
                    break;
                }
                keys.push(parent.clone());
                cursor = parent.clone();
            }
            keys.reverse();
            return Some(
                keys.into_iter()
                    .filter_map(|key| {
                        report
                            .callables
                            .get(&key)
                            .map(|item| item.display_name.clone())
                    })
                    .collect(),
            );
        }
        for next in &node.calls {
            if !previous.contains_key(next) {
                previous.insert(next.clone(), current.clone());
                queue.push_back(next.clone());
            }
        }
    }
    None
}
