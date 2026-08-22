fn canonical_decl_name(
    raw: &str,
    redirects: &HashMap<String, String>,
    type_redirects: &HashMap<String, String>,
) -> String {
    crate::ast::resolve_canonical_name(raw, None, &[Some(redirects)], type_redirects)
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
