
impl Checker {
    pub(crate) fn infer_and_enforce_authority(&mut self, program: &Program) {
        let mut seeds = Vec::<Seed>::new();
        let mut handlers = BTreeMap::<String, Vec<String>>::new();
        let mut static_schedules = HashMap::<String, Vec<Expr>>::new();
        let mut phases = HashMap::<String, Vec<String>>::new();
        let mut known_views = HashMap::new();
        collect_static_schedules(
            &program.declarations,
            &HashMap::new(),
            &mut static_schedules,
        );
        collect_phases(&program.declarations, &HashMap::new(), &mut phases);
        collect_view_names(&program.declarations, &HashMap::new(), &mut known_views);
        collect_seeds(
            &program.declarations,
            None,
            &HashMap::new(),
            &self.type_redirects,
            &mut seeds,
            &mut handlers,
        );
        for binding in crate::ast::canonical_module_bindings(&self.alias_decls) {
            let declarations = binding.declarations();
            let redirects = binding.local_redirects();
            collect_static_schedules(declarations, &redirects, &mut static_schedules);
            collect_phases(declarations, &redirects, &mut phases);
            collect_view_names(declarations, &redirects, &mut known_views);
            collect_seeds(
                declarations,
                // Canonical encoded namespaces remain the graph identity and
                // redirect target. Human-facing reports use the normalized
                // module identity so paths never expose implementation
                // symbols such as `p6c6f...`.
                Some(binding.module_identity()),
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
            .chain(known_views.keys())
            .cloned()
            .collect();
        let mut known_native_constructors =
            self.native_types.keys().cloned().collect::<HashSet<_>>();
        known_native_constructors.extend(
            crate::native_types::NativeScalarKind::ALL
                .into_iter()
                .map(|kind| kind.to_string()),
        );
        let allocation_free_fields = self
            .components
            .iter()
            .map(|(name, component)| {
                let fields = component
                    .fields
                    .iter()
                    .filter_map(|(field, ty)| {
                        matches!(
                            ty,
                            crate::types::Ty::Float
                                | crate::types::Ty::Bool
                                | crate::types::Ty::EntityId
                        )
                        .then_some(field.clone())
                    })
                    .collect();
                (name.clone(), fields)
            })
            .collect();
        let resolver = Resolver {
            known_callables,
            known_data,
            known_native_constructors,
            module_aliases: self.module_aliases.clone(),
            type_redirects: self.type_redirects.clone(),
            static_schedules,
            phases,
            known_views,
            allocation_free_fields,
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
            if !matches!(
                node.seed.kind,
                AuthorityCallableKind::System
                    | AuthorityCallableKind::Transaction
                    | AuthorityCallableKind::PostCommit
            ) {
                continue;
            }
            let specialized = specialize_system_graph(&base_nodes, &call_sites, &node.seed.name);
            let synchronous = freeze_graph(&specialized, false);
            match node.seed.kind {
                AuthorityCallableKind::System => self.enforce_system_authority(node, &synchronous),
                AuthorityCallableKind::Transaction => {
                    self.enforce_transaction_authority(node, &synchronous)
                }
                AuthorityCallableKind::PostCommit => {
                    self.enforce_post_commit_authority(node, &synchronous)
                }
                _ => unreachable!("only authority roots are specialized"),
            }
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
        self.enforce_write_ownership(program);
    }
}
