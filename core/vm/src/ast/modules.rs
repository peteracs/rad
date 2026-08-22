#[derive(Debug, Clone)]
pub struct Program {
    pub declarations: Vec<Decl>,
}

#[derive(Debug, Clone)]
pub enum Decl {
    Component(DataDecl),
    Resource(ResourceDecl),
    Struct(DataDecl),
    Intent(IntentDecl),
    Law(LawDecl),
    Resolver(ResolverDecl),
    Constraint(ConstraintDecl),
    Entity(EntityDecl),
    State(StateDecl),
    System(SystemDecl),
    Event(EventDecl),
    OnHandler(OnHandler),
    Migration(MigrationDecl),
    Phase(PhaseDecl),
    Fn(FnDecl),
    Type(TypeDeclNode),
    TypeAlias(TypeAliasDecl),
    NativeType(NativeTypeDecl),
    MaterializedView(MaterializedViewDecl),
    Use(UseStmt),
    Test(TestDecl),
    Model(ModelDecl),
    Stmt(Stmt),
    Error,
}

/// One lexical binding for a path-canonical module instance. Alias spelling
/// is deliberately absent from semantic identity: multiple aliases may bind
/// one `module_identity` and one namespace. `None` means that the same module
/// was also imported bare, so its declarations use the flat program names.
#[derive(Debug, Clone)]
pub struct ModuleAlias {
    declarations: Vec<Decl>,
    module_identity: String,
    canonical_namespace: Option<String>,
}

impl ModuleAlias {
    /// Build a path-identified module in its private canonical namespace.
    /// Alias spelling is deliberately not accepted as namespace input.
    pub fn namespaced(declarations: Vec<Decl>, module_identity: String) -> Self {
        let canonical_namespace = Some(canonical_module_namespace(&module_identity));
        Self {
            declarations,
            module_identity,
            canonical_namespace,
        }
    }

    /// Build an alias binding for a module that was also imported bare. Its
    /// declarations already occupy the flat program namespace.
    pub fn flattened(declarations: Vec<Decl>, module_identity: String) -> Self {
        Self {
            declarations,
            module_identity,
            canonical_namespace: None,
        }
    }

    pub fn declarations(&self) -> &[Decl] {
        &self.declarations
    }

    pub fn module_identity(&self) -> &str {
        &self.module_identity
    }

    pub fn canonical_namespace(&self) -> Option<&str> {
        self.canonical_namespace.as_deref()
    }

    /// This module's local name -> mangled name table.
    ///
    /// Four callers derived this by unwrapping `canonical_namespace()` with the
    /// same `expect`, then handing the namespace and declarations back to
    /// `module_local_redirects`. The binding already guarantees the namespace,
    /// so each caller was re-asserting an invariant it was handed.
    pub fn local_redirects(&self) -> std::collections::HashMap<String, String> {
        module_local_redirects(
            self.canonical_namespace()
                .expect("canonical bindings are namespaced"),
            self.declarations(),
        )
    }

    pub fn canonical_symbol(&self, member: &str) -> String {
        self.canonical_namespace().map_or_else(
            || member.to_string(),
            |namespace| canonical_module_symbol(namespace, member),
        )
    }
}

/// Reversible runtime-symbol encoding. This is not a hash: distinct semantic
/// module identities cannot collide.
pub fn canonical_module_namespace(module_identity: &str) -> String {
    use std::fmt::Write as _;

    let mut namespace = String::with_capacity(1 + module_identity.len() * 2);
    namespace.push('p');
    for byte in module_identity.bytes() {
        write!(&mut namespace, "{byte:02x}").expect("writing to String cannot fail");
    }
    namespace
}

pub fn canonical_module_symbol(namespace: &str, member: &str) -> String {
    format!("__mod_{namespace}__{member}")
}

impl std::ops::Deref for ModuleAlias {
    type Target = [Decl];

    fn deref(&self) -> &Self::Target {
        &self.declarations
    }
}

impl<'a> IntoIterator for &'a ModuleAlias {
    type Item = &'a Decl;
    type IntoIter = std::slice::Iter<'a, Decl>;

    fn into_iter(self) -> Self::IntoIter {
        self.declarations.iter()
    }
}

/// Return each namespaced module exactly once in semantic-identity order.
/// Bare-imported modules are already present in `Program::declarations` and
/// therefore do not appear here.
pub fn canonical_module_bindings(
    aliases: &std::collections::HashMap<String, ModuleAlias>,
) -> Vec<&ModuleAlias> {
    let mut by_identity = std::collections::BTreeMap::new();
    for binding in aliases.values() {
        if binding.canonical_namespace().is_some() {
            by_identity
                .entry(binding.module_identity())
                .or_insert(binding);
        }
    }
    by_identity.into_values().collect()
}

pub fn public_module_members(
    aliases: &std::collections::HashMap<String, ModuleAlias>,
) -> std::collections::HashMap<String, std::collections::HashMap<String, String>> {
    aliases
        .iter()
        .map(|(alias, binding)| {
            let members = binding
                .declarations()
                .iter()
                .filter(|declaration| declaration.is_public())
                .filter_map(|declaration| declaration.namespace_name())
                .map(|name| (name.to_string(), binding.canonical_symbol(name)))
                .collect();
            (alias.clone(), members)
        })
        .collect()
}

pub fn module_local_redirects(
    namespace: &str,
    declarations: &[Decl],
) -> std::collections::HashMap<String, String> {
    let mut redirects = std::collections::HashMap::new();
    let mut insert = |name: &str| {
        redirects.insert(name.to_string(), canonical_module_symbol(namespace, name));
    };
    for declaration in declarations {
        if let Some(name) = declaration.namespace_name() {
            insert(name);
            continue;
        }
        match declaration {
            Decl::Stmt(Stmt::Let(binding)) => {
                for name in &binding.names {
                    insert(name);
                }
            }
            Decl::Stmt(Stmt::LetElse(binding)) => {
                if let Some(name) = binding.primary_binding_name() {
                    insert(&name);
                }
            }
            _ => {}
        }
    }
    redirects
}

pub fn resolve_canonical_name(
    name: &str,
    module_aliases: Option<
        &std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    >,
    active_redirects: &[Option<&std::collections::HashMap<String, String>>],
    type_redirects: &std::collections::HashMap<String, String>,
) -> String {
    let mut current = name.to_string();
    if let Some((alias, member)) = name.split_once('.') {
        if let Some(resolved) = module_aliases
            .and_then(|aliases| aliases.get(alias))
            .and_then(|members| members.get(member))
        {
            current.clone_from(resolved);
        }
    } else {
        for redirects in active_redirects.iter().flatten() {
            if let Some(resolved) = redirects.get(name) {
                current.clone_from(resolved);
                break;
            }
        }
    }

    // Redirect cycles are checker defects, but name resolution must still be
    // total. An acyclic map can traverse at most len() edges.
    for _ in 0..=type_redirects.len() {
        let Some(canonical) = type_redirects.get(&current) else {
            break;
        };
        current.clone_from(canonical);
    }
    current
}

impl Decl {
    /// Name occupying the ordinary module namespace. Tests, models,
    /// handlers, imports, migrations, and statement bindings have separate
    /// registration rules and deliberately return `None`.
    pub fn namespace_name(&self) -> Option<&str> {
        match self {
            Decl::Component(declaration) | Decl::Struct(declaration) => Some(&declaration.name),
            Decl::Resource(declaration) => Some(&declaration.name),
            Decl::Intent(declaration) => Some(&declaration.name),
            Decl::Law(declaration) => Some(&declaration.name),
            Decl::Resolver(declaration) => Some(&declaration.name),
            Decl::Constraint(declaration) => Some(&declaration.name),
            Decl::Entity(declaration) => Some(&declaration.name),
            Decl::State(declaration) => Some(&declaration.name),
            Decl::System(declaration) => Some(&declaration.name),
            Decl::Event(declaration) => Some(&declaration.name),
            Decl::Phase(declaration) => Some(&declaration.name),
            Decl::Fn(declaration) => Some(&declaration.name),
            Decl::Type(declaration) => Some(&declaration.name),
            Decl::TypeAlias(declaration) => Some(&declaration.name),
            Decl::NativeType(declaration) => Some(&declaration.name),
            Decl::MaterializedView(declaration) => Some(&declaration.name),
            Decl::OnHandler(_)
            | Decl::Migration(_)
            | Decl::Use(_)
            | Decl::Test(_)
            | Decl::Model(_)
            | Decl::Stmt(_)
            | Decl::Error => None,
        }
    }

    /// Clone a declaration with its ordinary module-namespace name replaced.
    ///
    /// Canonical module loading used to rename selected declaration kinds in
    /// each semantic pass. Keeping the exhaustive rename beside
    /// [`Self::namespace_name`] gives every pass one complete operation and
    /// makes a new named declaration a compile-time decision.
    pub(crate) fn with_namespace_name(&self, replacement: &str) -> Self {
        let mut declaration = self.clone();
        match &mut declaration {
            Decl::Component(value) | Decl::Struct(value) => {
                value.name = replacement.to_string()
            }
            Decl::Resource(value) => value.name = replacement.to_string(),
            Decl::Intent(value) => value.name = replacement.to_string(),
            Decl::Law(value) => value.name = replacement.to_string(),
            Decl::Resolver(value) => value.name = replacement.to_string(),
            Decl::Constraint(value) => value.name = replacement.to_string(),
            Decl::Entity(value) => value.name = replacement.to_string(),
            Decl::State(value) => value.name = replacement.to_string(),
            Decl::System(value) => value.name = replacement.to_string(),
            Decl::Event(value) => value.name = replacement.to_string(),
            Decl::Phase(value) => value.name = replacement.to_string(),
            Decl::Fn(value) => value.name = replacement.to_string(),
            Decl::Type(value) => value.name = replacement.to_string(),
            Decl::TypeAlias(value) => value.name = replacement.to_string(),
            Decl::NativeType(value) => value.name = replacement.to_string(),
            Decl::MaterializedView(value) => value.name = replacement.to_string(),
            Decl::OnHandler(_)
            | Decl::Migration(_)
            | Decl::Use(_)
            | Decl::Test(_)
            | Decl::Model(_)
            | Decl::Stmt(_)
            | Decl::Error => {}
        }
        declaration
    }

    pub fn is_public(&self) -> bool {
        match self {
            Decl::Component(declaration) | Decl::Struct(declaration) => declaration.is_pub,
            Decl::Resource(declaration) => declaration.is_pub,
            Decl::Intent(declaration) => declaration.is_pub,
            Decl::Law(declaration) => declaration.is_pub,
            Decl::Resolver(declaration) => declaration.is_pub,
            Decl::Constraint(declaration) => declaration.is_pub,
            Decl::Entity(declaration) => declaration.is_pub,
            Decl::State(declaration) => declaration.is_pub,
            Decl::System(declaration) => declaration.is_pub,
            Decl::Event(declaration) => declaration.is_pub,
            Decl::Phase(declaration) => declaration.is_pub,
            Decl::Fn(declaration) => declaration.is_pub,
            Decl::Type(declaration) => declaration.is_pub,
            Decl::TypeAlias(declaration) => declaration.is_pub,
            Decl::NativeType(declaration) => declaration.is_pub,
            Decl::MaterializedView(declaration) => declaration.is_pub,
            Decl::Stmt(Stmt::Let(binding)) => binding.is_pub,
            Decl::OnHandler(_)
            | Decl::Migration(_)
            | Decl::Use(_)
            | Decl::Test(_)
            | Decl::Model(_)
            | Decl::Stmt(_)
            | Decl::Error => false,
        }
    }

    /// Symbol participating in top-level duplicate detection. Public lets
    /// preserve their existing single exported binding contract; declaration
    /// forms use the same namespace metadata as checker and compiler aliases.
    pub fn namespace_symbol(&self) -> Option<(&str, &Span)> {
        if let Some(name) = self.namespace_name() {
            return self.span().map(|span| (name, span));
        }
        match self {
            Decl::Stmt(Stmt::Let(binding)) if binding.is_pub => binding
                .names
                .first()
                .map(|name| (name.as_str(), &binding.span)),
            _ => None,
        }
    }

    pub fn span(&self) -> Option<&Span> {
        match self {
            Decl::Component(c) => Some(&c.span),
            Decl::Resource(r) => Some(&r.span),
            Decl::Struct(s) => Some(&s.span),
            Decl::Intent(i) => Some(&i.span),
            Decl::Law(l) => Some(&l.span),
            Decl::Resolver(r) => Some(&r.span),
            Decl::Constraint(c) => Some(&c.span),
            Decl::Entity(e) => Some(&e.span),
            Decl::State(s) => Some(&s.span),
            Decl::System(s) => Some(&s.span),
            Decl::Event(e) => Some(&e.span),
            Decl::OnHandler(o) => Some(&o.span),
            Decl::Migration(m) => Some(&m.span),
            Decl::Phase(p) => Some(&p.span),
            Decl::Fn(f) => Some(&f.span),
            Decl::Type(t) => Some(&t.span),
            Decl::TypeAlias(a) => Some(&a.span),
            Decl::NativeType(n) => Some(&n.span),
            Decl::MaterializedView(v) => Some(&v.span),
            Decl::Use(u) => Some(&u.span),
            Decl::Test(t) => Some(&t.span),
            Decl::Model(m) => Some(&m.span),
            Decl::Stmt(s) => Some(s.span()),
            Decl::Error => None,
        }
    }
}
