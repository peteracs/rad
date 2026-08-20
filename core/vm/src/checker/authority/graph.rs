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

        connect_callbacks(&mut nodes, call_sites);
        connect_handlers(&mut nodes, &handlers);
        let enforcement_report = freeze_graph(&nodes, false);
        self.enforce_system_authority(&nodes, &enforcement_report);
        let mut report = freeze_graph(&nodes, true);
        for (name, callable) in &mut report.callables {
            if let Some(synchronous) = enforcement_report.callables.get(name) {
                callable.synchronous = synchronous.transitive.clone();
            }
        }
        self.authority = report;
    }

    fn enforce_system_authority(
        &mut self,
        nodes: &BTreeMap<String, NodeDraft>,
        report: &AuthorityReport,
    ) {
        for node in nodes.values() {
            if node.seed.kind != AuthorityCallableKind::System {
                continue;
            }
            let Some(inferred) = report.callables.get(&node.seed.name) else {
                continue;
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
            let reads_all = allowed_reads.contains(WHOLE_WORLD);
            let writes_all = allowed_writes.contains(WHOLE_WORLD);
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
            if denied_reads.is_empty() && denied_writes.is_empty() && !inferred.transitive.unknown {
                continue;
            }
            let mut violations = Vec::new();
            if !denied_reads.is_empty() {
                violations.push(format!("reads [{}]", denied_reads.join(", ")));
            }
            if !denied_writes.is_empty() {
                violations.push(format!("writes [{}]", denied_writes.join(", ")));
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
                    inferred
                        .transitive
                        .unknown
                        .then(|| unknown_effect_path(report, &node.seed.name))
                        .flatten()
                })
                .expect("every transitive authority violation must have a direct call path");
            self.authority_error(
                &node.seed.span,
                format!(
                    "System '{}' exceeds its declared authority: {}",
                    node.seed.display_name,
                    violations.join("; ")
                ),
                Some(format!(
                    "authority path: {}. Add the required component/resource to the system signature (use `mut` for writes), or move the access behind a separately scheduled authority boundary",
                    evidence.join(" -> ")
                )),
            );
        }
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
    let mut declared = EffectDraft::default();
    declared.async_effect = function.is_async;
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
    let mut declared = EffectDraft::default();
    declared.async_effect = handler.is_async;
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
            node.calls.extend(known_targets);
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
                    node.calls.extend(targets);
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
