impl Checker {
    pub(super) fn collect_declarations(&mut self, program: &Program) {
        for decl in &program.declarations {
            self.register_declaration(decl, None);
        }
    }

    /// Register one declaration under either its source name or a canonical
    /// module symbol. Main-program and namespaced-module registration used to
    /// be separate exhaustive matches: adding a declaration could compile yet
    /// silently behave differently through an alias. One dispatch owns both
    /// forms now; `canonical_name` changes identity, never declaration
    /// semantics.
    pub(super) fn register_declaration(
        &mut self,
        decl: &Decl,
        canonical_name: Option<&str>,
    ) {
        match decl {
            Decl::Component(component) => {
                let name = canonical_name.unwrap_or(&component.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    component.span.file,
                    &component.name,
                    Ty::Str,
                    &component.span,
                    component.is_pub,
                ) {
                    let mut declaration = component.clone();
                    declaration.name = name.to_string();
                    self.register_component(&declaration);
                    self.define(
                        name,
                        Ty::Str,
                        false,
                        component.span.clone(),
                        component.is_pub,
                        false,
                    );
                }
            }
            Decl::Resource(resource) => {
                let name = canonical_name.unwrap_or(&resource.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    resource.span.file,
                    &resource.name,
                    Ty::Str,
                    &resource.span,
                    resource.is_pub,
                ) {
                    let mut declaration = resource.clone();
                    declaration.name = name.to_string();
                    self.register_resource(&declaration);
                    self.define(
                        name,
                        Ty::Str,
                        false,
                        resource.span.clone(),
                        resource.is_pub,
                        false,
                    );
                }
            }
            Decl::Struct(structure) => {
                let name = canonical_name.unwrap_or(&structure.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    structure.span.file,
                    &structure.name,
                    Ty::Str,
                    &structure.span,
                    structure.is_pub,
                ) {
                    let mut declaration = structure.clone();
                    declaration.name = name.to_string();
                    self.register_struct(&declaration);
                    self.define(
                        name,
                        Ty::Str,
                        false,
                        structure.span.clone(),
                        structure.is_pub,
                        false,
                    );
                }
            }
            Decl::Intent(intent) => {
                let mut declaration = intent.clone();
                declaration.name = canonical_name.unwrap_or(&intent.name).to_string();
                self.register_intent(&declaration);
            }
            Decl::Law(law) => {
                let mut declaration = law.clone();
                declaration.name = canonical_name.unwrap_or(&law.name).to_string();
                self.register_law(&declaration);
            }
            Decl::Resolver(resolver) => {
                let mut declaration = resolver.clone();
                declaration.name = canonical_name.unwrap_or(&resolver.name).to_string();
                self.register_resolver(&declaration);
            }
            Decl::Constraint(constraint) => {
                let mut declaration = constraint.clone();
                declaration.name = canonical_name.unwrap_or(&constraint.name).to_string();
                self.register_constraint(&declaration);
            }
            Decl::State(state) => {
                let name = canonical_name.unwrap_or(&state.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    state.span.file,
                    &state.name,
                    Ty::Any,
                    &state.span,
                    state.is_pub,
                ) {
                    let mut declaration = state.clone();
                    declaration.name = name.to_string();
                    self.register_state_machine(&declaration);
                    self.define(
                        name,
                        Ty::Any,
                        false,
                        state.span.clone(),
                        state.is_pub,
                        false,
                    );
                }
            }
            Decl::System(system) => {
                let mut declaration = system.clone();
                declaration.name = canonical_name.unwrap_or(&system.name).to_string();
                self.register_system(&declaration);
            }
            Decl::Event(event) => {
                let name = canonical_name.unwrap_or(&event.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    event.span.file,
                    &event.name,
                    Ty::Str,
                    &event.span,
                    event.is_pub,
                ) {
                    let mut declaration = event.clone();
                    declaration.name = name.to_string();
                    self.register_event(&declaration);
                    self.define(
                        name,
                        Ty::Str,
                        false,
                        event.span.clone(),
                        event.is_pub,
                        false,
                    );
                }
            }
            Decl::Fn(function) => {
                let name = canonical_name.unwrap_or(&function.name);
                let mut declaration = function.clone();
                declaration.name = name.to_string();
                self.register_function(&declaration);
                if let Some(signature) = self.functions.get(name) {
                    let function_type = Ty::Fn {
                        params: signature.params.clone(),
                        ret: Box::new(signature.ret.clone()),
                        purity: if signature.effects.is_pure() {
                            FnPurity::Pure
                        } else if signature.effects.is_readonly() {
                            FnPurity::Readonly
                        } else {
                            FnPurity::Impure
                        },
                    };
                    self.define(
                        name,
                        function_type,
                        false,
                        function.span.clone(),
                        function.is_pub,
                        false,
                    );
                }
            }
            Decl::Type(sum_type) => {
                let name = canonical_name.unwrap_or(&sum_type.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    sum_type.span.file,
                    &sum_type.name,
                    Ty::Any,
                    &sum_type.span,
                    sum_type.is_pub,
                ) {
                    let mut declaration = sum_type.clone();
                    declaration.name = name.to_string();
                    self.register_sum_type(&declaration);
                    self.define(
                        name,
                        Ty::Any,
                        false,
                        sum_type.span.clone(),
                        sum_type.is_pub,
                        false,
                    );
                }
            }
            Decl::TypeAlias(alias) => {
                let name = canonical_name.unwrap_or(&alias.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    alias.span.file,
                    &alias.name,
                    Ty::Any,
                    &alias.span,
                    alias.is_pub,
                ) {
                    let mut declaration = alias.clone();
                    declaration.name = name.to_string();
                    self.register_type_alias(&declaration);
                    self.define(
                        name,
                        Ty::Any,
                        false,
                        alias.span.clone(),
                        alias.is_pub,
                        false,
                    );
                }
            }
            Decl::NativeType(native) => {
                let name = canonical_name.unwrap_or(&native.name);
                if !self.reuse_canonical_alias_type(
                    canonical_name,
                    native.span.file,
                    &native.name,
                    Ty::Any,
                    &native.span,
                    native.is_pub,
                ) {
                    let mut declaration = native.clone();
                    declaration.name = name.to_string();
                    self.register_native_type(&declaration);
                }
            }
            Decl::MaterializedView(view) => {
                let name = canonical_name.unwrap_or(&view.name);
                self.materialized_views.insert(name.to_string());
                self.define(
                    name,
                    Ty::Str,
                    false,
                    view.span.clone(),
                    view.is_pub,
                    false,
                );
            }
            Decl::Phase(phase) => {
                let name = canonical_name.unwrap_or(&phase.name).to_string();
                let systems = if canonical_name.is_some() {
                    phase
                        .systems
                        .iter()
                        .map(|system| self.resolve_canonical_name(system))
                        .collect()
                } else {
                    phase.systems.clone()
                };
                self.phases.insert(name, systems);
            }
            Decl::Entity(entity) => {
                let name = canonical_name.unwrap_or(&entity.name);
                self.define(
                    name,
                    Ty::EntityId,
                    false,
                    entity.span.clone(),
                    entity.is_pub,
                    false,
                );
            }
            Decl::OnHandler(_)
            | Decl::Migration(_)
            | Decl::Use(_)
            | Decl::Test(_)
            | Decl::Model(_)
            | Decl::Stmt(_)
            | Decl::Error => {}
        }
    }

    fn reuse_canonical_alias_type(
        &mut self,
        canonical_name: Option<&str>,
        file_id: Option<FileId>,
        source_name: &str,
        binding_type: Ty,
        span: &Span,
        is_public: bool,
    ) -> bool {
        let Some(alias_name) = canonical_name else {
            return false;
        };
        let Some(existing) = self.find_canonical_type_name(file_id, source_name) else {
            return false;
        };
        self.type_redirects
            .insert(alias_name.to_string(), existing);
        self.define(
            alias_name,
            binding_type,
            false,
            span.clone(),
            is_public,
            false,
        );
        true
    }
}
