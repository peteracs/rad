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
                if node.sync_emits.contains(&event) {
                    node.calls.extend(targets.iter().cloned());
                    node.plain_calls.extend(targets.iter().cloned());
                } else {
                    node.deferred_calls.extend(targets.iter().cloned());
                }
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
            contracts: (&node.seed.contracts).into(),
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

pub(super) fn effect_path(
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

pub(super) fn unknown_effect_path(report: &AuthorityReport, start: &str) -> Option<Vec<String>> {
    effect_path_matching(report, start, |effects| effects.unknown)
}

pub(super) fn effect_path_matching(
    report: &AuthorityReport,
    start: &str,
    matches: impl Fn(&AuthorityEffects) -> bool,
) -> Option<Vec<String>> {
    report.effect_path_matching(start, matches)
}
