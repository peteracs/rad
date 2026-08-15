

impl Compiler {
    fn component_fields_as_defaults(fields: &[FieldDef]) -> Vec<(String, Option<TypeExpr>, Expr)> {
        fields
            .iter()
            .map(|field| {
                (
                    field.name.clone(),
                    field.type_annotation.clone(),
                    field.default_value.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn should_optimize_egraph(&self, fn_name: &str) -> bool {
        let Some(output) = &self.checker_output else {
            return false;
        };

        let canonical_name = self.resolve_canonical_name(fn_name);
        [fn_name, canonical_name.as_str()].iter().any(|name| {
            output.functions.get(*name).is_some_and(|sig| {
                matches!(
                    &sig.effects,
                    EffectSet::Restricted(set)
                        if set.contains(&Effect::ECS) || set.contains(&Effect::ReadECS)
                )
            })
        })
    }

    /// Pure and read-only helpers are valid callees from settlements, laws,
    /// resolvers, and constraints. Compile them with the same conservative
    /// value lowering as their causal callers: a helper compiled earlier at
    /// top level must not hide an in-place heap opcode behind an otherwise
    /// pure call boundary.
    pub(crate) fn may_run_in_causal_region(&self, fn_name: &str) -> bool {
        let Some(output) = &self.checker_output else {
            return false;
        };
        let canonical_name = self.resolve_canonical_name(fn_name);
        [fn_name, canonical_name.as_str()].iter().any(|name| {
            output.functions.get(*name).is_some_and(|signature| {
                signature.effects.is_pure() || signature.effects.is_readonly()
            })
        })
    }

    pub fn new() -> Self {
        let main_scope = FnScope {
            chunk: Chunk::new("main"),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            settlement_depth: 0,
            loop_contexts: Vec::new(),
            last_get_local: HashMap::new(),
            unique_locals: std::collections::HashSet::new(),
            prev_instr_start: usize::MAX,
            label_high_water: 0,
        };
        let mut global_slots = HashMap::new();
        let mut global_names = Vec::new();
        for builtin in Builtin::ALL {
            let name = builtin.name().to_string();
            let slot = global_names.len() as u16;
            global_slots.insert(name.clone(), slot);
            global_names.push(name);
        }
        Self {
            functions: vec![main_scope],
            component_types: HashMap::new(),
            resource_types: HashMap::new(),
            chunks: Vec::new(),
            systems: Vec::new(),
            handlers: Vec::new(),
            migrations: Vec::new(),
            state_machines: Vec::new(),
            intent_types: HashMap::new(),
            resolvers: Vec::new(),
            constraints: Vec::new(),
            temp_counter: 0,
            global_mutability: HashMap::new(),
            for_iter_kinds: HashMap::new(),
            checker_components: HashMap::new(),
            checker_resources: HashMap::new(),
            checker_sum_types: HashMap::new(),
            type_redirects: HashMap::new(),
            variant_shorthand: std::collections::HashSet::new(),
            spread_lengths: HashMap::new(),
            global_slots,
            global_names,
            program_source_identity: None,
            module_aliases: HashMap::new(),
            alias_decls: HashMap::new(),
            current_alias_scope: None,
            file_private_scopes: HashMap::new(),
            current_file_scope: None,
            features: Vec::new(),
            release: false,
            warnings: Vec::new(),
            gc: GcHeap::new(),
            phases: HashMap::new(),
            serial_phases: Vec::new(),
            component_versions: HashMap::new(),
            declared_systems: std::collections::HashSet::new(),
            checker_output: None,
            allow_pipe_fusion: false,
            causal_lowering_depth: 0,
        }
    }

    pub(crate) fn in_causal_region(&self) -> bool {
        self.functions
            .last()
            .is_some_and(|scope| scope.settlement_depth > 0)
            || self.causal_lowering_depth > 0
    }

    pub fn with_release(mut self, release: bool) -> Self {
        self.release = release;
        self
    }

    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }

    /// Bind the compiler product to the authenticated source/module graph
    /// that produced it. This identity is semantic metadata for portable
    /// replay; it never changes bytecode generation.
    pub fn with_program_source_identity(mut self, identity: impl Into<String>) -> Self {
        self.program_source_identity = Some(identity.into());
        self
    }

    pub fn with_aliases(mut self, aliases: HashMap<String, Vec<Decl>>) -> Self {
        for (alias_name, decls) in &aliases {
            let mut pub_map = HashMap::new();
            for d in decls {
                if let Some(name) = Self::decl_name_static(d) {
                    if Self::decl_is_pub_static(d) {
                        let mangled = format!("__mod_{}__{}", alias_name, name);
                        pub_map.insert(name.to_string(), mangled);
                    }
                }
            }
            self.module_aliases.insert(alias_name.clone(), pub_map);
        }
        self.alias_decls = aliases;
        self
    }

    fn register_alias_local_names(
        names: &mut HashMap<String, String>,
        alias_name: &str,
        decl: &Decl,
    ) {
        if let Some(name) = Self::decl_name_static(decl) {
            names.insert(name.to_string(), format!("__mod_{}__{}", alias_name, name));
            return;
        }
        match decl {
            Decl::Stmt(Stmt::Let(binding)) => {
                for name in &binding.names {
                    names.insert(name.clone(), format!("__mod_{}__{}", alias_name, name));
                }
            }
            Decl::Stmt(Stmt::LetElse(binding)) => {
                if let Some(name) = binding.primary_binding_name() {
                    names.insert(name.clone(), format!("__mod_{}__{}", alias_name, name));
                }
            }
            _ => {}
        }
    }

    fn decl_name_static(decl: &Decl) -> Option<&str> {
        match decl {
            Decl::Component(c) => Some(&c.name),
            Decl::Resource(r) => Some(&r.name),
            Decl::Struct(s) => Some(&s.name),
            Decl::Intent(i) => Some(&i.name),
            Decl::Law(l) => Some(&l.name),
            Decl::Resolver(r) => Some(&r.name),
            Decl::Constraint(c) => Some(&c.name),
            Decl::Entity(e) => Some(&e.name),
            Decl::State(s) => Some(&s.name),
            Decl::System(s) => Some(&s.name),
            Decl::Event(e) => Some(&e.name),
            Decl::Phase(p) => Some(&p.name),
            Decl::Fn(f) => Some(&f.name),
            Decl::Type(t) => Some(&t.name),
            Decl::TypeAlias(a) => Some(&a.name),
            _ => None,
        }
    }

    fn decl_is_pub_static(decl: &Decl) -> bool {
        match decl {
            Decl::Component(c) => c.is_pub,
            Decl::Resource(r) => r.is_pub,
            Decl::Struct(s) => s.is_pub,
            Decl::Intent(i) => i.is_pub,
            Decl::Law(l) => l.is_pub,
            Decl::Resolver(r) => r.is_pub,
            Decl::Constraint(c) => c.is_pub,
            Decl::Entity(e) => e.is_pub,
            Decl::State(s) => s.is_pub,
            Decl::System(s) => s.is_pub,
            Decl::Event(e) => e.is_pub,
            Decl::Phase(p) => p.is_pub,
            Decl::Fn(f) => f.is_pub,
            Decl::Type(t) => t.is_pub,
            Decl::TypeAlias(a) => a.is_pub,
            Decl::Stmt(Stmt::Let(l)) => l.is_pub,
            _ => false,
        }
    }

    pub(crate) fn resolve_canonical_name(&self, name: &str) -> String {
        let mut current = name.to_string();
        if let Some(dot_pos) = current.find('.') {
            let alias = &current[..dot_pos];
            let member = &current[dot_pos + 1..];
            if let Some(alias_map) = self.module_aliases.get(alias) {
                if let Some(resolved) = alias_map.get(member) {
                    current = resolved.clone();
                }
            }
        } else if let Some(resolved) = self.resolve_current_alias(&current) {
            current = resolved;
        }
        while let Some(canonical) = self.type_redirects.get(&current) {
            current = canonical.clone();
        }
        current
    }

    pub(crate) fn resolve_alias_member(&self, alias: &str, member: &str) -> Option<String> {
        self.module_aliases
            .get(alias)
            .and_then(|m| m.get(member).cloned())
    }

    pub(crate) fn resolve_current_alias(&self, name: &str) -> Option<String> {
        if let Some(res) = self
            .current_alias_scope
            .as_ref()
            .and_then(|m| m.get(name).cloned())
        {
            return Some(res);
        }
        if let Some(res) = self
            .current_file_scope
            .as_ref()
            .and_then(|m| m.get(name).cloned())
        {
            return Some(res);
        }
        None
    }

    pub fn with_for_iter_kinds(mut self, hints: HashMap<NodeId, ForIterKind>) -> Self {
        self.for_iter_kinds = hints;
        self
    }

    pub fn with_checker_output(mut self, output: CheckerOutput) -> Self {
        self.checker_output = Some(output.clone());
        self.for_iter_kinds = output.for_iter_kinds;
        self.checker_components = output.components;
        self.checker_resources = output.resources;
        for (name, rs) in &self.checker_resources {
            self.checker_components.insert(
                name.clone(),
                ComponentType {
                    name: rs.name.clone(),
                    fields: rs.fields.clone(),
                    is_pub: rs.is_pub,
                    file_id: rs.file_id,
                    indexed_fields: std::collections::HashSet::new(),
                },
            );
        }
        for (name, st) in output.structs {
            self.checker_components.insert(
                name,
                ComponentType {
                    name: st.name,
                    fields: st.fields,
                    is_pub: st.is_pub,
                    file_id: st.file_id,
                    indexed_fields: std::collections::HashSet::new(),
                },
            );
        }
        self.checker_sum_types = output.sum_types;
        self.type_redirects = output.type_redirects;
        self.variant_shorthand = output.variant_shorthand;
        self.spread_lengths = output.spread_lengths;
        self
    }

    pub(crate) fn ensure_global_slot(&mut self, name: &str) -> u16 {
        let mut resolved = None;
        if let Some(ref scope) = self.current_alias_scope {
            resolved = scope.get(name).map(|s| s.as_str());
        }
        if resolved.is_none() {
            if let Some(ref scope) = self.current_file_scope {
                resolved = scope.get(name).map(|s| s.as_str());
            }
        }
        let effective = resolved.unwrap_or(name);
        if let Some(&slot) = self.global_slots.get(effective) {
            return slot;
        }
        let slot = self.global_names.len() as u16;
        self.global_slots.insert(effective.to_owned(), slot);
        self.global_names.push(effective.to_owned());
        slot
    }

    pub(crate) fn is_system(&self, name: &str) -> bool {
        // declared_systems covers the current program's declarations
        // position-independently; self.systems additionally holds systems
        // from alias modules (compiled before the main declaration loop).
        self.declared_systems.contains(name) || self.systems.iter().any(|s| s.name == name)
    }

    pub(crate) fn component_field_order(&self, comp_name: &str) -> Option<Vec<String>> {
        self.checker_components
            .get(comp_name)
            .map(|ct| ct.fields.iter().map(|(n, _)| n.clone()).collect())
    }

    fn compile_alias_decls(&mut self) -> Result<(), CompileError> {
        let alias_decls = std::mem::take(&mut self.alias_decls);
        for (alias_name, decls) in &alias_decls {
            let mut all_names: HashMap<String, String> = HashMap::new();
            for d in decls {
                Self::register_alias_local_names(&mut all_names, alias_name, d);
            }
            self.current_alias_scope = Some(all_names.clone());
            for d in decls {
                match d {
                    Decl::Component(c) => {
                        self.component_types.insert(
                            all_names
                                .get(&c.name)
                                .cloned()
                                .unwrap_or_else(|| c.name.clone()),
                            Self::component_fields_as_defaults(&c.fields),
                        );
                    }
                    Decl::Resource(r) => {
                        self.resource_types.insert(
                            all_names
                                .get(&r.name)
                                .cloned()
                                .unwrap_or_else(|| r.name.clone()),
                            Self::component_fields_as_defaults(&r.fields),
                        );
                    }
                    Decl::Struct(s) => {
                        self.component_types.insert(
                            all_names
                                .get(&s.name)
                                .cloned()
                                .unwrap_or_else(|| s.name.clone()),
                            Self::component_fields_as_defaults(&s.fields),
                        );
                    }
                    _ => {}
                }
            }
            for d in decls {
                self.compile_decl(d)?;
            }
            self.current_alias_scope = None;
        }
        self.alias_decls = alias_decls;
        Ok(())
    }

    pub(crate) fn new_fn_scope(name: &str) -> FnScope {
        FnScope {
            chunk: Chunk::new(name),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 1,
            settlement_depth: 0,
            loop_contexts: Vec::new(),
            last_get_local: HashMap::new(),
            unique_locals: std::collections::HashSet::new(),
            prev_instr_start: usize::MAX,
            label_high_water: 0,
        }
    }

    pub(crate) fn fresh_name(&mut self, prefix: &str) -> String {
        self.temp_counter += 1;
        format!("__{}{}", prefix, self.temp_counter)
    }

    pub fn compile(mut self, program: &Program) -> Result<CompileResult, CompileError> {
        let mut file_private_scopes: HashMap<u32, HashMap<String, String>> = HashMap::new();
        for decl in &program.declarations {
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if file_id.0 != 0 && !Self::decl_is_pub_static(decl) {
                        if let Some(name) = Self::decl_name_static(decl) {
                            let mangled = format!("__priv_{}__{}", file_id.0, name);
                            file_private_scopes
                                .entry(file_id.0)
                                .or_default()
                                .insert(name.to_string(), mangled);
                        }
                    }
                }
            }
        }
        self.file_private_scopes = file_private_scopes;

        for feature in &self.features.clone() {
            let name = format!("FEATURE_{}", feature.to_uppercase());
            let slot = self.ensure_global_slot(&name);
            self.emit_constant(Value::from_bool(true), 0);
            self.emit_op(Op::DefGlobal, 0);
            self.emit_u16(slot, 0);
        }

        let has_main_fn = program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "main"));

        for decl in &program.declarations {
            let mut resolved_name = None;
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if let Some(scope) = self.file_private_scopes.get(&file_id.0) {
                        if let Some(name) = Self::decl_name_static(decl) {
                            if let Some(mangled) = scope.get(name) {
                                resolved_name = Some(mangled.clone());
                            }
                        }
                    }
                }
            }

            match decl {
                Decl::Component(c) => {
                    self.component_types.insert(
                        resolved_name.unwrap_or_else(|| c.name.clone()),
                        Self::component_fields_as_defaults(&c.fields),
                    );
                }
                Decl::Resource(r) => {
                    self.resource_types.insert(
                        resolved_name.unwrap_or_else(|| r.name.clone()),
                        Self::component_fields_as_defaults(&r.fields),
                    );
                }
                Decl::Struct(s) => {
                    self.component_types.insert(
                        resolved_name.unwrap_or_else(|| s.name.clone()),
                        Self::component_fields_as_defaults(&s.fields),
                    );
                }
                _ => {}
            }
        }

        self.compile_alias_decls()?;

        // Declaration-metadata pre-pass, then hoist top-level `fn`
        // definitions ahead of every other declaration. The checker places
        // every top-level fn in scope everywhere and the docs promise
        // forward references work; without hoisting the binding only exists
        // once execution reaches the `fn` statement, so an earlier call
        // trapped on `nil`. Hoisting is observation-free: a top-level fn
        // decl only emits DefGlobal of a constant fn value (top-level fns
        // capture no upvalues — main's top-level lets are globals, not
        // locals), so entity-spawn order and statement effects are
        // unchanged. Compiling fn bodies first is only correct because the
        // pre-pass has already registered every later declaration's
        // compile-time facts: which names are systems, which globals are
        // immutable, which names are phases.
        for decl in &program.declarations {
            self.predeclare_decl_metadata(decl);
        }
        for decl in &program.declarations {
            if matches!(
                decl,
                Decl::Fn(_) | Decl::Law(_) | Decl::Resolver(_) | Decl::Constraint(_)
            ) {
                self.compile_decl(decl)?;
            }
        }
        for decl in &program.declarations {
            if !matches!(
                decl,
                Decl::Fn(_) | Decl::Law(_) | Decl::Resolver(_) | Decl::Constraint(_)
            ) {
                self.compile_decl(decl)?;
            }
        }

        let layout_analysis = if let Some(output) = &self.checker_output {
            layout_analysis::LayoutAnalysis::analyze(output, |name| {
                self.resolve_canonical_name(name)
            })
        } else {
            layout_analysis::LayoutAnalysis::default()
        };

        if has_main_fn {
            let line = 0;
            let main_slot = self.ensure_global_slot("main");
            self.emit_op(Op::GetGlobal, line);
            self.emit_u16(main_slot, line);
            self.emit_op(Op::Call, line);
            self.emit_byte(0, line);
            self.emit_op(Op::PopCheckErr, line);
        }

        self.emit_op(Op::Halt, 0);
        let main_chunk = self.functions.pop().unwrap().chunk;
        let mut result = vec![main_chunk];
        result.extend(self.chunks);

        let mut component_layouts = HashMap::new();
        let mut component_field_types = HashMap::new();
        let mut indexed_component_fields = HashMap::new();
        let mut transient_resources = std::collections::HashSet::new();
        for (name, (_, fields)) in &self.intent_types {
            component_layouts.insert(Self::intent_runtime_type(name), fields.clone());
        }
        for (name, ct) in &self.checker_components {
            component_layouts.insert(
                name.clone(),
                ct.fields
                    .iter()
                    .map(|(n, _)| n.clone())
                    .collect::<Vec<String>>(),
            );
            component_field_types.insert(name.clone(), ct.fields.clone());
            indexed_component_fields.insert(
                name.clone(),
                ct.indexed_fields.iter().cloned().collect::<Vec<String>>(),
            );
        }
        for decl in &program.declarations {
            let mut resolved_name = None;
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if let Some(scope) = self.file_private_scopes.get(&file_id.0) {
                        if let Some(name) = Self::decl_name_static(decl) {
                            if let Some(mangled) = scope.get(name) {
                                resolved_name = Some(mangled.clone());
                            }
                        }
                    }
                }
            }

            match decl {
                Decl::Event(e) => {
                    component_layouts.insert(
                        resolved_name.unwrap_or_else(|| e.name.clone()),
                        e.fields.iter().map(|(n, _)| n.clone()).collect(),
                    );
                }
                Decl::Component(c) => {
                    let name = resolved_name.unwrap_or_else(|| c.name.clone());
                    component_layouts.insert(
                        name.clone(),
                        c.fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<String>>(),
                    );
                    // `indexed` declarations must survive checker-less
                    // compiles (replay of embedded trace source) — the AST
                    // is the source of truth, the checker copy is a cache.
                    indexed_component_fields
                        .entry(name)
                        .or_insert_with(|| c.indexed_fields.clone());
                }
                Decl::Resource(r) => {
                    let name = resolved_name.unwrap_or_else(|| r.name.clone());
                    if r.transient {
                        transient_resources.insert(name.clone());
                    }
                    component_layouts.insert(
                        name,
                        r.fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<String>>(),
                    );
                }
                Decl::Struct(s) => {
                    component_layouts.insert(
                        resolved_name.unwrap_or_else(|| s.name.clone()),
                        s.fields
                            .iter()
                            .map(|field| field.name.clone())
                            .collect::<Vec<String>>(),
                    );
                }
                _ => {}
            }
        }
        for (alias_name, decls) in &self.alias_decls {
            for decl in decls {
                match decl {
                    Decl::Event(e) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, e.name),
                            e.fields.iter().map(|(n, _)| n.clone()).collect(),
                        );
                    }
                    Decl::Component(c) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, c.name),
                            c.fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect::<Vec<String>>(),
                        );
                    }
                    Decl::Resource(r) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, r.name),
                            r.fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect::<Vec<String>>(),
                        );
                    }
                    Decl::Struct(s) => {
                        component_layouts.insert(
                            format!("__mod_{}__{}", alias_name, s.name),
                            s.fields
                                .iter()
                                .map(|field| field.name.clone())
                                .collect::<Vec<String>>(),
                        );
                    }
                    _ => {}
                }
            }
        }
        let mut variant_layouts = HashMap::new();
        for (type_name, stdef) in &self.checker_sum_types {
            for variant in &stdef.variants {
                let key = (type_name.clone(), variant.name.clone());
                variant_layouts
                    .insert(key, variant.fields.iter().map(|(n, _)| n.clone()).collect());
            }
        }

        let materialization_plan =
            materialization::MaterializationPlan::from_layout_analysis(&layout_analysis);

        // Stamp serial-phase membership onto the compiled systems (dogfood
        // feature seq 83). Done here — not at phase-compile time — because a
        // `serial phase` may be declared before or after its member systems.
        // A system in several serial phases keeps the first group; groups
        // only ever ADD conflicts, so the batches stay correct either way.
        for (gid, (_phase_name, members)) in self.serial_phases.iter().enumerate() {
            for sys in &mut self.systems {
                if members.contains(&sys.name) && sys.serial_group.is_none() {
                    sys.serial_group = Some(gid as u32);
                }
            }
        }

        Ok(CompileResult {
            chunks: result,
            systems: self.systems,
            handlers: self.handlers,
            migrations: self.migrations,
            state_machines: self.state_machines,
            intents: self
                .intent_types
                .iter()
                .map(|(name, (key_field, fields))| IntentChunkInfo {
                    name: name.clone(),
                    key_field: key_field.clone(),
                    fields: fields.clone(),
                })
                .collect(),
            resolvers: self.resolvers,
            constraints: self.constraints,
            layout_analysis,
            materialization_plan,
            component_layouts,
            component_field_types,
            indexed_component_fields,
            transient_resources,
            component_versions: std::mem::take(&mut self.component_versions),
            variant_layouts,
            global_names: self.global_names,
            program_source_identity: self.program_source_identity,
            warnings: std::mem::take(&mut self.warnings),
            gc: std::mem::take(&mut self.gc),
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}
