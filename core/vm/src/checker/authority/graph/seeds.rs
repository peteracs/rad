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
            Decl::Fn(function) => {
                seeds.push(fn_seed(function, alias, redirects));
                for statement in &function.body.stmts {
                    if let Stmt::Transaction(transaction) = statement {
                        seeds.push(transaction_seed(
                            function,
                            transaction,
                            alias,
                            redirects,
                            type_redirects,
                        ));
                        if transaction.post_commit.is_some() {
                            seeds.push(post_commit_seed(function, transaction, alias, redirects));
                        }
                    }
                }
            }
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

fn collect_view_names(
    declarations: &[Decl],
    redirects: &HashMap<String, String>,
    names: &mut HashMap<String, Vec<String>>,
) {
    for declaration in declarations {
        if let Decl::MaterializedView(view) = declaration {
            let name = redirects
                .get(&view.name)
                .cloned()
                .unwrap_or_else(|| view.name.clone());
            let dependencies = view
                .dependencies
                .iter()
                .map(|dependency| {
                    redirects
                        .get(dependency)
                        .cloned()
                        .unwrap_or_else(|| dependency.clone())
                })
                .collect();
            names.insert(name, dependencies);
        }
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
        contracts: CallableContracts::default(),
        transaction_changes_only: None,
        declared,
        captured_locals: HashMap::new(),
    }
}

fn transaction_seed(
    function: &FnDecl,
    transaction: &crate::ast::TransactionStmt,
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
    type_redirects: &HashMap<String, String>,
) -> Seed {
    let function_name = redirects
        .get(&function.name)
        .cloned()
        .unwrap_or_else(|| function.name.clone());
    let file = transaction.span.file.map_or(0, |file| file.0);
    let name = format!(
        "@transaction:{function_name}:{file}:{}:{}",
        transaction.span.line, transaction.span.col
    );
    let display_name = alias.map_or_else(
        || format!("transaction {}", transaction.name),
        |alias| format!("transaction {alias}.{}", transaction.name),
    );
    let mut isolated = transaction.clone();
    isolated.post_commit = None;
    Seed {
        name,
        display_name,
        kind: AuthorityCallableKind::Transaction,
        block: Block {
            id: transaction.id,
            span: transaction.span.clone(),
            stmts: vec![Stmt::Transaction(isolated)],
        },
        span: transaction.span.clone(),
        redirects: redirects.clone(),
        params: function.params.clone(),
        param_bounds: callable_param_bounds(&function.param_types),
        system_params: BTreeMap::new(),
        authority_reads: BTreeSet::new(),
        authority_writes: BTreeSet::new(),
        authority_emits: BTreeSet::new(),
        authority_io: false,
        authority_async: false,
        contracts: CallableContracts::default(),
        transaction_changes_only: Some(
            transaction
                .changes_only
                .iter()
                .map(|authority| canonical_decl_name(authority, redirects, type_redirects))
                .collect(),
        ),
        declared: EffectDraft::default(),
        captured_locals: HashMap::new(),
    }
}

fn post_commit_seed(
    function: &FnDecl,
    transaction: &crate::ast::TransactionStmt,
    alias: Option<&str>,
    redirects: &HashMap<String, String>,
) -> Seed {
    let function_name = redirects
        .get(&function.name)
        .cloned()
        .unwrap_or_else(|| function.name.clone());
    let file = transaction.span.file.map_or(0, |file| file.0);
    Seed {
        name: format!(
            "@post_commit:{function_name}:{file}:{}:{}",
            transaction.span.line, transaction.span.col
        ),
        display_name: alias.map_or_else(
            || format!("post_commit {}", transaction.name),
            |alias| format!("post_commit {alias}.{}", transaction.name),
        ),
        kind: AuthorityCallableKind::PostCommit,
        block: transaction
            .post_commit
            .clone()
            .expect("post_commit seed requires a block"),
        span: transaction.span.clone(),
        redirects: redirects.clone(),
        params: function.params.clone(),
        param_bounds: callable_param_bounds(&function.param_types),
        system_params: BTreeMap::new(),
        authority_reads: BTreeSet::new(),
        authority_writes: BTreeSet::new(),
        authority_emits: BTreeSet::new(),
        authority_io: false,
        authority_async: false,
        contracts: CallableContracts::default(),
        transaction_changes_only: None,
        declared: EffectDraft::default(),
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
        contracts: system.contracts.clone(),
        transaction_changes_only: None,
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
        contracts: handler.contracts.clone(),
        transaction_changes_only: None,
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
        contracts: CallableContracts::default(),
        transaction_changes_only: None,
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
        contracts: CallableContracts::default(),
        transaction_changes_only: None,
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
