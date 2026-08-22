use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ast::*;
use crate::visitor::{self, AstVisitor};

use super::Checker;

#[derive(Clone)]
struct OwnershipPolicy {
    name: String,
    owner_file: Option<FileId>,
    owner_module: Option<String>,
    owner_module_identity: Option<String>,
    whole: bool,
    fields: BTreeSet<String>,
    coowner_modules: BTreeSet<String>,
    coowner_module_identities: BTreeSet<String>,
}

impl OwnershipPolicy {
    fn controls(&self, field: Option<&str>) -> bool {
        self.whole || field.is_none() || field.is_some_and(|name| self.fields.contains(name))
    }
}

#[derive(Clone)]
struct WriteUse {
    type_name: String,
    field: Option<String>,
    span: Span,
    operation: &'static str,
}

struct WriteCollector<'a> {
    writes: Vec<WriteUse>,
    mutable_system_params: &'a HashMap<String, String>,
    typed_values: HashMap<String, String>,
    despawns: Vec<Span>,
}

impl<'a> WriteCollector<'a> {
    fn new(
        mutable_system_params: &'a HashMap<String, String>,
        typed_values: HashMap<String, String>,
    ) -> Self {
        Self {
            writes: Vec::new(),
            mutable_system_params,
            typed_values,
            despawns: Vec::new(),
        }
    }

    fn whole(&mut self, type_name: impl Into<String>, span: &Span, operation: &'static str) {
        self.writes.push(WriteUse {
            type_name: type_name.into(),
            field: None,
            span: span.clone(),
            operation,
        });
    }

    fn field(
        &mut self,
        type_name: impl Into<String>,
        field: impl Into<String>,
        span: &Span,
        operation: &'static str,
    ) {
        self.writes.push(WriteUse {
            type_name: type_name.into(),
            field: Some(field.into()),
            span: span.clone(),
            operation,
        });
    }

    fn component_expr(&mut self, expr: &Expr, operation: &'static str) -> bool {
        match expr {
            Expr::ComponentExpr(type_name, _, _, span) => {
                self.whole(type_name.clone(), span, operation);
                true
            }
            Expr::Ident(value_name, span) => {
                let Some(type_name) = self.typed_values.get(value_name).cloned() else {
                    return false;
                };
                self.whole(type_name, span, operation);
                true
            }
            _ => false,
        }
    }

    fn component_entry(&mut self, entry: &ComponentEntry, operation: &'static str) {
        match entry {
            ComponentEntry::Init(init) => self.whole(&init.comp_name, &init.span, operation),
            ComponentEntry::Expr(expr) => {
                self.component_expr(expr, operation);
            }
        }
    }

    fn assigned_system_field(&mut self, target: &Expr, span: &Span) {
        let (root, field) = match target {
            Expr::Field(root, field, _) => (root.as_ref(), Some(field.as_str())),
            Expr::Index(root, _, _) => match root.as_ref() {
                Expr::Field(root, field, _) => (root.as_ref(), Some(field.as_str())),
                other => (other, None),
            },
            _ => return,
        };
        let Expr::Ident(param, _) = root else {
            return;
        };
        let Some(type_name) = self.mutable_system_params.get(param) else {
            return;
        };
        if let Some(field) = field {
            self.field(type_name, field, span, "mutable system writeback");
        } else {
            self.whole(type_name, span, "mutable system writeback");
        }
    }
}

impl AstVisitor for WriteCollector<'_> {
    fn visit_block(&mut self, block: &Block) {
        let outer_values = self.typed_values.clone();
        visitor::walk_block(self, block);
        self.typed_values = outer_values;
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Assign(assign) => self.assigned_system_field(&assign.target, &assign.span),
            Stmt::Update(update) => {
                for field in &update.field_updates {
                    self.field(&update.comp_name, &field.name, &update.span, "update");
                }
            }
            Stmt::Next(next) => {
                self.whole(&next.component_name, &next.span, "resolver replacement")
            }
            _ => {}
        }
        visitor::walk_stmt(self, stmt);
        if let Stmt::Let(binding) = stmt {
            let inferred = match &binding.type_annotation {
                Some(TypeExpr::Named(type_name)) => Some(type_name.clone()),
                _ => match &binding.value {
                    Expr::ComponentExpr(type_name, _, _, _) => Some(type_name.clone()),
                    Expr::Ident(value_name, _) => self.typed_values.get(value_name).cloned(),
                    _ => None,
                },
            };
            if let Some(type_name) = inferred {
                for name in &binding.names {
                    self.typed_values.insert(name.clone(), type_name.clone());
                }
            } else {
                for name in &binding.names {
                    self.typed_values.remove(name);
                }
            }
        }
    }

    fn visit_call_expr(&mut self, callee: &Expr, args: &[Expr], span: &Span) {
        if let Expr::Ident(name, _) = callee {
            match name.as_str() {
                "set" => {
                    if let Some(value) = args.get(1) {
                        self.component_expr(value, "set");
                    }
                }
                "write_field" => {
                    if let (
                        Some(Expr::Ident(type_name, _) | Expr::StrLit(type_name, _)),
                        Some(Expr::StrLit(field, _)),
                    ) = (args.get(1), args.get(2))
                    {
                        self.field(type_name, field, span, "write_field");
                    }
                }
                "remove" => {
                    if let Some(Expr::Ident(type_name, _) | Expr::StrLit(type_name, _)) =
                        args.get(1)
                    {
                        self.whole(type_name, span, "remove");
                    }
                }
                "set_resource" => {
                    if let Some(type_expr) = args.first() {
                        match type_expr {
                            Expr::Ident(type_name, _) | Expr::StrLit(type_name, _) => {
                                self.whole(type_name, span, "set_resource");
                            }
                            _ => {
                                if let Some(value) = args.get(1) {
                                    self.component_expr(value, "set_resource");
                                }
                            }
                        }
                    }
                }
                "spawn" => {
                    for value in args {
                        self.component_expr(value, "spawn");
                    }
                }
                "despawn" => self.despawns.push(span.clone()),
                _ => {}
            }
        }
        visitor::walk_call_expr(self, callee, args);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let Expr::EntityLiteral(_, components, _) = expr {
            for component in components {
                self.component_entry(component, "entity construction");
            }
        }
        visitor::walk_expr(self, expr);
    }
}

struct Scope<'a> {
    name: String,
    module_identity: Option<&'a str>,
    file: Option<FileId>,
    grants: &'a [String],
    test_only: bool,
    implicit_owner: bool,
    mutable_system_params: HashMap<String, String>,
    typed_values: HashMap<String, String>,
}

impl Checker {
    pub(super) fn enforce_write_ownership(&mut self, program: &Program) {
        let mut policies = BTreeMap::<String, OwnershipPolicy>::new();
        self.collect_ownership_policies(&program.declarations, &mut policies);
        for binding in crate::ast::canonical_module_bindings(&self.alias_decls) {
            self.collect_ownership_policies(binding.declarations(), &mut policies);
        }
        if policies.is_empty() {
            return;
        }

        let known_aliases = self.alias_decls.keys().cloned().collect::<BTreeSet<_>>();
        for policy in policies.values() {
            if let Some(owner) = &policy.owner_module {
                if !known_aliases.contains(owner) {
                    self.authority_error(
                        &Span {
                            line: 1,
                            col: 1,
                            file: policy.owner_file,
                        },
                        format!(
                            "Owned type '{}' transfers ownership to unknown module '{}'",
                            policy.name, owner
                        ),
                        Some("Import the destination with `use \"...\" as NAME`".to_string()),
                    );
                }
            }
            for coowner in &policy.coowner_modules {
                if !known_aliases.contains(coowner) {
                    self.authority_error(
                        &Span {
                            line: 1,
                            col: 1,
                            file: policy.owner_file,
                        },
                        format!(
                            "Owned type '{}' names unknown co-owner module '{}'",
                            policy.name, coowner
                        ),
                        Some(
                            "Import the co-owner with `use \"...\" as NAME` before naming it"
                                .to_string(),
                        ),
                    );
                }
            }
        }

        self.check_owned_declarations(&program.declarations, None, None, &policies);
        let aliases = self.alias_decls.clone();
        for binding in crate::ast::canonical_module_bindings(&aliases) {
            let namespace = binding
                .canonical_namespace()
                .expect("canonical bindings are namespaced");
            self.check_owned_declarations(
                binding.declarations(),
                Some(namespace),
                Some(binding.module_identity()),
                &policies,
            );
        }
    }

    fn collect_ownership_policies(
        &self,
        declarations: &[Decl],
        policies: &mut BTreeMap<String, OwnershipPolicy>,
    ) {
        for declaration in declarations {
            let (name, span, ownership, fields) = match declaration {
                Decl::Component(component) => (
                    &component.name,
                    &component.span,
                    &component.ownership,
                    component.fields.as_slice(),
                ),
                Decl::Resource(resource) => (
                    &resource.name,
                    &resource.span,
                    &resource.ownership,
                    resource.fields.as_slice(),
                ),
                _ => continue,
            };
            let owned_fields = fields
                .iter()
                .filter(|field| field.is_owned)
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            if !ownership.owned && owned_fields.is_empty() {
                continue;
            }
            let canonical = self.canonical_ownership_type(span.file, name);
            let owner_module = ownership.transferred_to.clone();
            let owner_module_identity = owner_module
                .as_ref()
                .and_then(|module| self.alias_decls.get(module))
                .map(|binding| binding.module_identity().to_string());
            let owner_file = owner_module
                .as_ref()
                .and_then(|module| self.alias_decls.get(module))
                .and_then(|declarations| declarations.iter().find_map(Decl::span))
                .and_then(|span| span.file)
                .or(span.file);
            policies
                .entry(canonical.clone())
                .or_insert_with(|| OwnershipPolicy {
                    name: canonical,
                    owner_file,
                    owner_module,
                    owner_module_identity,
                    whole: ownership.owned,
                    fields: owned_fields,
                    coowner_modules: ownership.coowner_modules.iter().cloned().collect(),
                    coowner_module_identities: ownership
                        .coowner_modules
                        .iter()
                        .filter_map(|module| self.alias_decls.get(module))
                        .map(|binding| binding.module_identity().to_string())
                        .collect(),
                });
        }
    }

    fn canonical_ownership_type(&self, file: Option<FileId>, raw: &str) -> String {
        if raw.contains('.') {
            return self.resolve_canonical_name(raw);
        }
        self.find_canonical_type_name(file, raw)
            .unwrap_or_else(|| self.resolve_canonical_name(raw))
    }

    fn check_owned_declarations(
        &mut self,
        declarations: &[Decl],
        alias: Option<&str>,
        module_identity: Option<&str>,
        policies: &BTreeMap<String, OwnershipPolicy>,
    ) {
        let mut handler_index = HashMap::<String, usize>::new();
        for declaration in declarations {
            match declaration {
                Decl::Fn(function) => {
                    let typed_values = function
                        .params
                        .iter()
                        .zip(&function.param_types)
                        .filter_map(|(name, type_expr)| match type_expr {
                            Some(TypeExpr::Named(type_name)) => {
                                Some((name.clone(), type_name.clone()))
                            }
                            _ => None,
                        })
                        .collect();
                    let scope = Scope {
                        name: alias.map_or_else(
                            || function.name.clone(),
                            |module| format!("{module}.{}", function.name),
                        ),
                        module_identity,
                        file: function.span.file,
                        grants: &function.ownership_writes,
                        test_only: false,
                        implicit_owner: false,
                        mutable_system_params: HashMap::new(),
                        typed_values,
                    };
                    self.check_owned_block(&scope, &function.body, policies);
                }
                Decl::System(system) => {
                    let typed_values = system
                        .params
                        .iter()
                        .map(|(name, _, type_name)| (name.clone(), type_name.clone()))
                        .collect();
                    let mutable_system_params = system
                        .params
                        .iter()
                        .filter(|(_, mutable, _)| *mutable)
                        .map(|(name, _, type_name)| (name.clone(), type_name.clone()))
                        .collect();
                    let scope = Scope {
                        name: alias.map_or_else(
                            || system.name.clone(),
                            |module| format!("{module}.{}", system.name),
                        ),
                        module_identity,
                        file: system.span.file,
                        grants: &system.ownership_writes,
                        test_only: false,
                        implicit_owner: false,
                        mutable_system_params,
                        typed_values,
                    };
                    self.check_owned_block(&scope, &system.body, policies);
                }
                Decl::OnHandler(handler) => {
                    let count = handler_index.entry(handler.event_name.clone()).or_default();
                    *count += 1;
                    let scope = Scope {
                        name: format!("on {}#{}", handler.event_name, count),
                        module_identity,
                        file: handler.span.file,
                        grants: &handler.ownership_writes,
                        test_only: false,
                        implicit_owner: false,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    self.check_owned_block(&scope, &handler.body, policies);
                }
                Decl::Test(test) => {
                    let scope = Scope {
                        name: format!("test '{}'", test.name),
                        module_identity,
                        file: test.span.file,
                        grants: &test.ownership_writes,
                        test_only: true,
                        implicit_owner: false,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    self.check_owned_block(&scope, &test.body, policies);
                }
                Decl::Model(model) => {
                    let scope = Scope {
                        name: format!("model '{}'", model.name),
                        module_identity,
                        file: model.span.file,
                        grants: &model.ownership_writes,
                        test_only: true,
                        implicit_owner: false,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    let mut collector = WriteCollector::new(
                        &scope.mutable_system_params,
                        scope.typed_values.clone(),
                    );
                    for command in &model.commands {
                        collector.visit_expr(command);
                    }
                    for invariant in &model.invariants {
                        collector.visit_block(invariant);
                    }
                    self.enforce_collected_writes(&scope, collector, policies);
                }
                Decl::Resolver(resolver) => {
                    let scope = Scope {
                        name: format!("resolver {}", resolver.name),
                        module_identity,
                        file: resolver.span.file,
                        grants: &[],
                        test_only: false,
                        implicit_owner: true,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    self.check_owned_block(&scope, &resolver.body, policies);
                }
                Decl::Migration(migration) => {
                    let scope = Scope {
                        name: format!("migration {}", migration.component),
                        module_identity,
                        file: migration.span.file,
                        grants: &[],
                        test_only: false,
                        implicit_owner: true,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    self.check_owned_block(&scope, &migration.body, policies);
                }
                Decl::Entity(entity) => {
                    let no_mutable_params = HashMap::new();
                    let mut collector = WriteCollector::new(&no_mutable_params, HashMap::new());
                    for component in &entity.components {
                        collector.component_entry(component, "entity declaration");
                    }
                    let scope = Scope {
                        name: format!("entity {}", entity.name),
                        module_identity,
                        file: entity.span.file,
                        grants: &[],
                        test_only: false,
                        implicit_owner: true,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    self.enforce_collected_writes(&scope, collector, policies);
                }
                Decl::Stmt(statement) => {
                    let scope = Scope {
                        name: "top-level statement".to_string(),
                        module_identity,
                        file: statement.span().file,
                        grants: &[],
                        test_only: false,
                        implicit_owner: false,
                        mutable_system_params: HashMap::new(),
                        typed_values: HashMap::new(),
                    };
                    let mut collector = WriteCollector::new(
                        &scope.mutable_system_params,
                        scope.typed_values.clone(),
                    );
                    collector.visit_stmt(statement);
                    self.enforce_collected_writes(&scope, collector, policies);
                }
                _ => {}
            }
        }
    }

    fn check_owned_block(
        &mut self,
        scope: &Scope<'_>,
        block: &Block,
        policies: &BTreeMap<String, OwnershipPolicy>,
    ) {
        let mut collector =
            WriteCollector::new(&scope.mutable_system_params, scope.typed_values.clone());
        for type_name in scope.mutable_system_params.values() {
            collector.whole(type_name, &block.span, "mutable system parameter");
        }
        collector.visit_block(block);
        self.enforce_collected_writes(scope, collector, policies);
    }

    fn enforce_collected_writes(
        &mut self,
        scope: &Scope<'_>,
        mut collector: WriteCollector<'_>,
        policies: &BTreeMap<String, OwnershipPolicy>,
    ) {
        for span in collector.despawns.drain(..) {
            for policy in policies.values() {
                collector.writes.push(WriteUse {
                    type_name: policy.name.clone(),
                    field: None,
                    span: span.clone(),
                    operation: "despawn",
                });
            }
        }

        let grants = self.resolve_owned_grants(scope, policies);
        let mut emitted = BTreeSet::new();
        for write in collector.writes {
            let canonical = self.canonical_ownership_type(write.span.file, &write.type_name);
            let Some(policy) = policies.get(&canonical) else {
                continue;
            };
            if !policy.controls(write.field.as_deref()) {
                continue;
            }
            let is_owner = policy.owner_module_identity.as_ref().map_or_else(
                || scope.file == policy.owner_file,
                |module| scope.module_identity == Some(module.as_str()),
            );
            let authorized_module = scope.test_only
                || is_owner
                || scope
                    .module_identity
                    .is_some_and(|module| policy.coowner_module_identities.contains(module));
            let granted = scope.implicit_owner && is_owner
                || grants.contains("*")
                || grants.contains(&canonical)
                || write
                    .field
                    .as_ref()
                    .is_some_and(|field| grants.contains(&format!("{canonical}.{field}")));
            let identity = (
                write.span.line,
                write.span.col,
                canonical.clone(),
                write.field.clone(),
            );
            if authorized_module && granted || !emitted.insert(identity) {
                continue;
            }
            let target = write
                .field
                .as_ref()
                .map_or_else(|| canonical.clone(), |field| format!("{canonical}.{field}"));
            let owner = policy.owner_file.map_or_else(
                || "the declaring source module".to_string(),
                |file| format!("source module #{}", file.0),
            );
            let reason = if !authorized_module {
                format!("{} is write-owned by {owner}", policy.name)
            } else {
                format!("{} has no owned-write grant for {target}", scope.name)
            };
            self.authority_error(
                &write.span,
                format!(
                    "Ownership violation: {} cannot {} '{}': {reason}",
                    scope.name, write.operation, target
                ),
                Some(format!(
                    "Declare `writes owned [{target}]` on an authorized callable; reads remain public"
                )),
            );
        }
    }

    fn resolve_owned_grants(
        &mut self,
        scope: &Scope<'_>,
        policies: &BTreeMap<String, OwnershipPolicy>,
    ) -> BTreeSet<String> {
        let mut resolved = BTreeSet::new();
        for raw in scope.grants {
            if raw == "*" {
                resolved.insert(raw.clone());
                continue;
            }
            let target = if let Some((prefix, suffix)) = raw.split_once('.') {
                if self.module_aliases.contains_key(prefix) {
                    self.resolve_canonical_name(raw)
                } else {
                    let canonical = self.canonical_ownership_type(scope.file, prefix);
                    format!("{canonical}.{suffix}")
                }
            } else {
                self.canonical_ownership_type(scope.file, raw)
            };
            let type_name = target
                .split_once('.')
                .map_or(target.as_str(), |pair| pair.0);
            let Some(policy) = policies.get(type_name) else {
                self.authority_error(
                    &Span {
                        line: 1,
                        col: 1,
                        file: scope.file,
                    },
                    format!(
                        "Owned-write grant '{}' on {} does not name an owned component or resource",
                        raw, scope.name
                    ),
                    Some(
                        "Remove the stale grant or mark the target declaration/field `owned`"
                            .to_string(),
                    ),
                );
                continue;
            };
            let is_owner = policy.owner_module_identity.as_ref().map_or_else(
                || scope.file == policy.owner_file,
                |module| scope.module_identity == Some(module.as_str()),
            );
            let authorized_module = scope.test_only
                || is_owner
                || scope
                    .module_identity
                    .is_some_and(|module| policy.coowner_module_identities.contains(module));
            if !authorized_module {
                self.authority_error(
                    &Span {
                        line: 1,
                        col: 1,
                        file: scope.file,
                    },
                    format!(
                        "{} cannot acquire owned-write capability '{}'; the module is not the owner or a declared co-owner",
                        scope.name, raw
                    ),
                    None,
                );
                continue;
            }
            resolved.insert(target);
        }
        resolved
    }
}
