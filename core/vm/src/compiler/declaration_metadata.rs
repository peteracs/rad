use super::*;

/// Compiler metadata sourced directly from declarations before any body is
/// lowered.
///
/// Entry modules and canonical modules previously populated default-bearing
/// type schemas through two matches, then populated runtime layouts through
/// another two. The alias layout path had already drifted once by omitting
/// `transient resource`. One exhaustive recorder now owns both projections;
/// only canonical-name resolution differs between callers.
#[derive(Default)]
pub(super) struct DeclarationMetadata {
    pub component_types: HashMap<String, Vec<(String, Option<TypeExpr>, Expr)>>,
    pub resource_types: HashMap<String, Vec<(String, Option<TypeExpr>, Expr)>>,
    pub component_layouts: HashMap<String, Vec<String>>,
    pub indexed_component_fields: HashMap<String, Vec<String>>,
    pub ordered_component_fields: HashMap<String, Vec<String>>,
    pub transient_resources: std::collections::HashSet<String>,
}

impl DeclarationMetadata {
    fn record(&mut self, declaration: &Decl, canonical_name: &str) {
        match declaration {
            Decl::Event(event) => {
                self.component_layouts.insert(
                    canonical_name.to_string(),
                    event.fields.iter().map(|(name, _)| name.clone()).collect(),
                );
            }
            Decl::Component(component) => {
                let name = canonical_name.to_string();
                self.component_types.insert(
                    name.clone(),
                    Compiler::component_fields_as_defaults(&component.fields),
                );
                self.component_layouts.insert(
                    name.clone(),
                    component
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
                        .collect(),
                );
                self.indexed_component_fields
                    .insert(name.clone(), component.indexed_fields.clone());
                self.ordered_component_fields
                    .insert(name, component.ordered_indexed_fields.clone());
            }
            Decl::Resource(resource) => {
                let name = canonical_name.to_string();
                self.resource_types.insert(
                    name.clone(),
                    Compiler::component_fields_as_defaults(&resource.fields),
                );
                if resource.transient {
                    self.transient_resources.insert(name.clone());
                }
                self.component_layouts.insert(
                    name,
                    resource
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
                        .collect(),
                );
            }
            Decl::Struct(structure) => {
                let name = canonical_name.to_string();
                self.component_types.insert(
                    name.clone(),
                    Compiler::component_fields_as_defaults(&structure.fields),
                );
                self.component_layouts.insert(
                    name,
                    structure
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
                        .collect(),
                );
            }
            Decl::Intent(_)
            | Decl::Law(_)
            | Decl::Resolver(_)
            | Decl::Constraint(_)
            | Decl::Entity(_)
            | Decl::State(_)
            | Decl::System(_)
            | Decl::OnHandler(_)
            | Decl::Migration(_)
            | Decl::Phase(_)
            | Decl::Fn(_)
            | Decl::Type(_)
            | Decl::TypeAlias(_)
            | Decl::NativeType(_)
            | Decl::MaterializedView(_)
            | Decl::Use(_)
            | Decl::Test(_)
            | Decl::Model(_)
            | Decl::Stmt(_)
            | Decl::Error => {}
        }
    }
}

impl Compiler {
    fn file_scoped_declaration_name(&self, declaration: &Decl) -> Option<String> {
        let source_name = declaration.namespace_name()?;
        Some(
            declaration
                .span()
                .and_then(|span| span.file)
                .and_then(|file_id| self.file_private_scopes.get(&file_id.0))
                .and_then(|scope| scope.get(source_name))
                .map_or_else(|| source_name.to_string(), Clone::clone),
        )
    }

    pub(super) fn collect_declaration_metadata(&self, program: &Program) -> DeclarationMetadata {
        let mut metadata = DeclarationMetadata::default();
        for declaration in &program.declarations {
            if let Some(name) = self.file_scoped_declaration_name(declaration) {
                metadata.record(declaration, &name);
            }
        }
        for binding in crate::ast::canonical_module_bindings(&self.alias_decls) {
            for declaration in binding.declarations() {
                if let Some(source_name) = declaration.namespace_name() {
                    metadata.record(declaration, &binding.canonical_symbol(source_name));
                }
            }
        }
        metadata
    }
}
