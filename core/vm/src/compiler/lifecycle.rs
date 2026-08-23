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

    pub(crate) fn should_optimize_expressions(&self, fn_name: &str) -> bool {
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
            function_declarations: HashMap::new(),
            materialized_views: Vec::new(),
            view_kernels: Vec::new(),
            indexed_kernel_fields: std::collections::HashSet::new(),
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
            native_types: HashMap::new(),
            variant_shorthand: std::collections::HashSet::new(),
            spread_lengths: HashMap::new(),
            global_slots,
            global_names,
            shared_world_tests: std::collections::HashSet::new(),
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
            expected_checker_options: None,
            authority: None,
            authority_errors: Vec::new(),
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
        self.features = features.clone();
        if let Some(options) = &mut self.expected_checker_options {
            options.features = features;
        }
        self
    }

    pub fn with_checker_options(mut self, options: crate::checker::CheckerOptions) -> Self {
        self.features = options.features.clone();
        self.expected_checker_options = Some(options);
        self
    }

    /// Bind the compiler product to the authenticated source/module graph
    /// that produced it. This identity is semantic metadata for portable
    /// replay; it never changes bytecode generation.
    pub fn with_program_source_identity(mut self, identity: impl Into<String>) -> Self {
        self.program_source_identity = Some(identity.into());
        self
    }

    pub fn with_aliases(mut self, aliases: HashMap<String, ModuleAlias>) -> Self {
        self.module_aliases = crate::ast::public_module_members(&aliases);
        self.alias_decls = aliases;
        self
    }

    pub(crate) fn resolve_canonical_name(&self, name: &str) -> String {
        crate::ast::resolve_canonical_name(
            name,
            Some(&self.module_aliases),
            &[
                self.current_alias_scope.as_ref(),
                self.current_file_scope.as_ref(),
            ],
            &self.type_redirects,
        )
    }

    pub(crate) fn add_canonical_name_constant(&mut self, name: &str) -> u16 {
        let resolved = self.resolve_canonical_name(name);
        self.add_constant_gc(|gc| Value::from_string(gc, resolved))
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

    pub fn with_checker_output(mut self, output: CheckerOutput) -> Self {
        self.checker_output = Some(output);
        self
    }

    fn install_checker_output(&mut self, output: CheckerOutput) {
        self.authority = Some(output.authority.clone());
        self.authority_errors = output.authority_errors.clone();
        self.for_iter_kinds = output.for_iter_kinds.clone();
        self.checker_components = output.components.clone();
        self.checker_resources = output.resources.clone();
        for (name, rs) in &self.checker_resources {
            self.checker_components.insert(
                name.clone(),
                ComponentType::from_declared_fields(
                    rs.name.clone(),
                    rs.fields.clone(),
                    rs.is_pub,
                    rs.file_id,
                ),
            );
        }
        for (name, st) in &output.structs {
            self.checker_components.insert(
                name.clone(),
                ComponentType::from_declared_fields(
                    st.name.clone(),
                    st.fields.clone(),
                    st.is_pub,
                    st.file_id,
                ),
            );
        }
        self.checker_sum_types = output.sum_types.clone();
        self.type_redirects = output.type_redirects.clone();
        self.variant_shorthand = output.variant_shorthand.clone();
        self.spread_lengths = output.spread_lengths.clone();
        self.checker_output = Some(output);
    }

    fn semantic_product_error(program: &Program, message: impl Into<String>) -> CompileError {
        let span = program
            .declarations
            .first()
            .and_then(Decl::span)
            .cloned()
            .unwrap_or_default();
        CompileError {
            message: message.into(),
            line: span.line,
            col: span.col,
        }
    }

    fn normalized_checker_options(
        options: &crate::checker::CheckerOptions,
    ) -> crate::checker::CheckerOptions {
        let mut normalized = options.clone();
        normalized.features.sort();
        normalized.features.dedup();
        normalized
    }

    fn validate_checker_output(
        &self,
        program: &Program,
        output: &CheckerOutput,
    ) -> Result<(), CompileError> {
        let Some(expected_input) = output.semantic_input_fingerprint else {
            return Err(Self::semantic_product_error(
                program,
                "Checker output is not a checked semantic product; run Checker::check before compiling",
            ));
        };
        let Some(options) = output.semantic_options.as_ref() else {
            return Err(Self::semantic_product_error(
                program,
                "Checker output is missing its semantic configuration",
            ));
        };
        let Some(expected_product) = output.product_fingerprint else {
            return Err(Self::semantic_product_error(
                program,
                "Checker output is missing its semantic product integrity digest",
            ));
        };
        if crate::types::semantic_product_fingerprint(output) != expected_product {
            return Err(Self::semantic_product_error(
                program,
                "Checker output failed semantic product integrity validation",
            ));
        }
        let actual_input =
            crate::types::semantic_program_fingerprint(program, &self.alias_decls, options);
        if actual_input != expected_input {
            return Err(Self::semantic_product_error(
                program,
                "Checker output belongs to a different program or module graph, or was produced under a different semantic configuration; check and compile the same semantic input",
            ));
        }

        let mut checked_features = options.features.clone();
        checked_features.sort();
        checked_features.dedup();
        let mut compiler_features = self.features.clone();
        compiler_features.sort();
        compiler_features.dedup();
        if checked_features != compiler_features {
            return Err(Self::semantic_product_error(
                program,
                "Checker output was produced under a different enabled-feature configuration",
            ));
        }
        if let Some(expected_options) = &self.expected_checker_options {
            if Self::normalized_checker_options(expected_options)
                != Self::normalized_checker_options(options)
            {
                return Err(Self::semantic_product_error(
                    program,
                    "Checker output was produced under different checker semantic options",
                ));
            }
        }
        Ok(())
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
        for binding in crate::ast::canonical_module_bindings(&alias_decls) {
            let decls = binding.declarations();
            let all_names = binding.local_redirects();
            self.current_alias_scope = Some(all_names.clone());
            // Namespaced modules obey the same declaration semantics as the
            // entry module. Register every compile-time fact before lowering
            // any body so forward references, system classification, and
            // view-kernel callback analysis cannot depend on source order.
            for declaration in decls {
                self.predeclare_decl_metadata(declaration);
            }
            for d in decls {
                if Self::compiles_in_first_pass(d) {
                    self.compile_decl(d)?;
                }
            }
            for d in decls {
                if !Self::compiles_in_first_pass(d) {
                    self.compile_decl(d)?;
                }
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

    fn compiles_in_first_pass(declaration: &Decl) -> bool {
        matches!(
            declaration,
            Decl::Fn(_)
                | Decl::Law(_)
                | Decl::Resolver(_)
                | Decl::Constraint(_)
                | Decl::NativeType(_)
                | Decl::MaterializedView(_)
        )
    }

    pub fn compile(mut self, program: &Program) -> Result<CompileResult, CompileError> {
        if let Some(output) = self.checker_output.take() {
            self.validate_checker_output(program, &output)?;
            self.install_checker_output(output);
        }
        // Authority is mandatory even for direct compiler callers. Keep the
        // inferred graph separate from the optional full checker product so
        // authority enforcement does not silently enable unrelated typed
        // lowering and persisted-world schema validation.
        if self.authority.is_none() {
            let mut checker =
                crate::checker::Checker::new_with_options(crate::checker::CheckerOptions {
                    features: self.features.clone(),
                    ..crate::checker::CheckerOptions::default()
                });
            checker.set_aliases(self.alias_decls.clone());
            let _ = checker.check(program);
            // Authority only: `output()` would clone every checker map and
            // fingerprint the result to hand back two fields.
            let (authority, authority_errors) = checker.authority_report();
            self.authority_errors = authority_errors;
            self.authority = Some(authority);
        }
        if let Some(error) = self.authority_errors.first() {
            return Err(CompileError {
                message: error.hint.as_ref().map_or_else(
                    || error.message.clone(),
                    |hint| format!("{}\n  hint: {}", error.message, hint),
                ),
                line: error.line,
                col: error.col,
            });
        }

        let mut file_private_scopes: HashMap<u32, HashMap<String, String>> = HashMap::new();
        for decl in &program.declarations {
            if let Some(span) = decl.span() {
                if let Some(file_id) = span.file {
                    if file_id.0 != 0 && !decl.is_public() {
                        if let Some(name) = decl.namespace_name() {
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

        // Fixed-width scalar constructors are real runtime type values, not
        // aliases or checker-only syntax. Install them before any user code.
        for repr in crate::native_types::NativeScalarKind::ALL {
            let name = repr.to_string();
            let slot = self.ensure_global_slot(&name);
            self.emit_constant_gc(0, |gc| {
                Value::from_native_type(gc, crate::native_types::NativeTypeDescriptor::scalar(repr))
            });
            self.emit_op(Op::DefGlobal, 0);
            self.emit_u16(slot, 0);
        }

        let has_main_fn = program
            .declarations
            .iter()
            .any(|d| matches!(d, Decl::Fn(f) if f.name == "main"));

        let mut declaration_metadata = self.collect_declaration_metadata(program);
        self.component_types
            .extend(std::mem::take(&mut declaration_metadata.component_types));
        self.resource_types
            .extend(std::mem::take(&mut declaration_metadata.resource_types));

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
            if Self::compiles_in_first_pass(decl) {
                self.compile_decl(decl)?;
            }
        }
        for decl in &program.declarations {
            if !Self::compiles_in_first_pass(decl) {
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
        let native_layouts =
            crate::native_types::compute_native_layouts(program).map_err(|message| {
                CompileError {
                    message,
                    line: 0,
                    col: 0,
                }
            })?;

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
        let mut ordered_component_fields = HashMap::new();
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
        component_layouts.extend(declaration_metadata.component_layouts);
        indexed_component_fields.extend(declaration_metadata.indexed_component_fields);
        ordered_component_fields.extend(declaration_metadata.ordered_component_fields);
        transient_resources.extend(declaration_metadata.transient_resources);
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
            materialized_views: self.materialized_views,
            view_kernels: self.view_kernels,
            layout_analysis,
            materialization_plan,
            component_layouts,
            component_field_types,
            indexed_component_fields,
            ordered_component_fields,
            native_layouts,
            transient_resources,
            component_versions: std::mem::take(&mut self.component_versions),
            variant_layouts,
            shared_world_tests: std::mem::take(&mut self.shared_world_tests),
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
