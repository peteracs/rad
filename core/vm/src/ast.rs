#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

pub struct NodeIdGen {
    next: u32,
}

impl NodeIdGen {
    pub fn new() -> Self {
        Self { next: 0 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> NodeId {
        let id = NodeId(self.next);
        self.next += 1;
        id
    }
}

impl Default for NodeIdGen {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub source: String,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(path: String, source: String) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            path,
            source,
            line_starts,
        }
    }

    pub fn line_col(&self, offset: u32) -> (u32, u32) {
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let col = offset - self.line_starts[line];
        ((line + 1) as u32, col + 1)
    }

    pub fn line_text(&self, line: u32) -> &str {
        let idx = (line as usize).saturating_sub(1);
        if idx >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[idx] as usize;
        let end = if idx + 1 < self.line_starts.len() {
            (self.line_starts[idx + 1] as usize).saturating_sub(1)
        } else {
            self.source.len()
        };
        &self.source[start..end]
    }
}

#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self { files: Vec::new() }
    }

    pub fn add_file(&mut self, path: String, source: String) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(path, source));
        id
    }

    pub fn get_file(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn resolve_span(&self, span: &Span) -> Option<(&str, u32, u32)> {
        let file_id = span.file?;
        let file = self.get_file(file_id)?;
        Some((&file.path, span.line, span.col))
    }

    pub fn file_id_for_path(&self, path: &str) -> Option<FileId> {
        self.files
            .iter()
            .position(|f| f.path == path)
            .map(|i| FileId(i as u32))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub file: Option<FileId>,
}

impl Span {
    pub fn with_file(mut self, file: FileId) -> Self {
        self.file = Some(file);
        self
    }
}

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
    Use(UseStmt),
    Test(TestDecl),
    Stmt(Stmt),
    Error,
}

impl Decl {
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
            Decl::Use(u) => Some(&u.span),
            Decl::Test(t) => Some(&t.span),
            Decl::Stmt(s) => Some(s.span()),
            Decl::Error => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub body: Block,
    pub is_property: bool,
    pub generators: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub struct QueryExprNode {
    pub components: Vec<(String, bool)>,
    pub filter: Option<Box<Expr>>,
    pub select: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub type_annotation: Option<TypeExpr>,
    pub default_value: Expr,
    pub is_indexed: bool,
    /// Annotation-only fields (`source: entity` — no `=`): the type has no
    /// sensible zero value, so every construction must provide one. The
    /// `default_value` holds a nil placeholder for layout machinery.
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ResourceDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    /// `transient resource` — runtime state excluded from the world's
    /// identity: world_digest() and save_world() skip it (command
    /// tapes, derived caches, spatial indexes).
    pub transient: bool,
    /// `resource X v2 { … }` — declared schema version, embedded per type
    /// in `save_world()` output and handed to `migrate X(old, from_version)`
    /// on load (dogfood feature seq 69 IDEA 03). 0 = undeclared.
    pub version: u32,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone)]
pub struct EntityDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub components: Vec<ComponentEntry>,
}

#[derive(Debug, Clone)]
pub struct ComponentInit {
    pub id: NodeId,
    pub span: Span,
    pub comp_name: String,
    pub fields: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub enum ComponentEntry {
    Init(ComponentInit),
    Expr(Expr),
}

impl ComponentEntry {
    pub fn as_init(&self) -> Option<&ComponentInit> {
        match self {
            ComponentEntry::Init(ci) => Some(ci),
            _ => None,
        }
    }

    pub fn as_expr(&self) -> Option<&Expr> {
        match self {
            ComponentEntry::Expr(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub states: Vec<StateDef>,
}

#[derive(Debug, Clone)]
pub struct StateDef {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub transitions: Vec<(String, String, Option<Expr>)>,
}

#[derive(Debug, Clone)]
pub struct SystemDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<(String, bool, String)>,
    /// Names of params declared `accum` (dogfood feature seq 83 IDEA 02).
    /// They also appear in `params` with `is_mut = true` — `accum` is `mut`
    /// plus fold-on-merge semantics for parallel batches.
    pub accum_params: Vec<String>,
    pub body: Block,
    pub after: Vec<String>,
    pub before: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EventDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    /// Field name and optional type (`event E { a, b: int }`). Untyped fields infer as `any` in the checker.
    pub fields: Vec<(String, Option<TypeExpr>)>,
}

/// A transient proposal schema owned by one resolver in its defining module.
#[derive(Debug, Clone)]
pub struct IntentDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub fields: Vec<IntentField>,
}

#[derive(Debug, Clone)]
pub struct IntentField {
    pub span: Span,
    pub name: String,
    pub type_annotation: TypeExpr,
    pub is_key: bool,
}

/// A read-only producer that can only be invoked from a settlement.
#[derive(Debug, Clone)]
pub struct LawDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<String>,
    pub param_types: Vec<TypeExpr>,
    pub body: Block,
}

/// The single semantic owner of one intent type.
#[derive(Debug, Clone)]
pub struct ResolverDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub intent_name: String,
    pub key_param: String,
    pub proposals_param: String,
    pub body: Block,
}

/// A validation-only invariant over one complete settlement candidate.
#[derive(Debug, Clone)]
pub struct ConstraintDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub component_name: String,
    pub subject_param: String,
    pub proposed_param: String,
    pub watches: Vec<String>,
    pub body: Block,
}

/// Schema migration (list item #5): `migrate Health(old) { return Health { … } }`.
/// Invoked by `load_world` when a persisted component/resource shape differs
/// from the declared one. `param_name` binds the stored fields as a
/// `map<str, any>` — the old shape no longer exists as a type.
#[derive(Debug, Clone)]
pub struct MigrationDecl {
    pub id: NodeId,
    pub span: Span,
    pub component: String,
    pub param_name: String,
    /// `migrate X(old, from_version)` — optional second parameter binding
    /// the schema version the SAVE declared for `X` (`component X v2 { … }`
    /// at save time), or 0 for saves without one (dogfood feature seq 69
    /// IDEA 03: turn shape-sniffing into a fact).
    pub version_param: Option<String>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct OnHandler {
    pub id: NodeId,
    pub span: Span,
    pub event_name: String,
    pub param_name: String,
    pub body: Block,
    pub once: bool,
    pub is_async: bool,
    pub has_guard: bool,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub params: Vec<String>,
    pub param_muts: Vec<bool>,
    pub param_types: Vec<Option<TypeExpr>>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
    pub is_pure: bool,
    pub is_async: bool,
    pub effects: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub target: TypeExpr,
}

#[derive(Debug, Clone)]
pub struct TypeDeclNode {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub type_params: Vec<String>,
    pub variants: Vec<VariantDefNode>,
}

#[derive(Debug, Clone)]
pub struct VariantDefNode {
    pub name: String,
    pub fields: Vec<(String, Expr)>,
    /// Fields declared with a TYPE annotation instead of a default value
    /// (`Homing { target: entity }`). Sparse: only annotated names appear;
    /// their `fields` entry carries a nil placeholder default.
    pub annotations: Vec<(String, TypeExpr)>,
}

#[derive(Debug, Clone)]
pub struct UseStmt {
    pub id: NodeId,
    pub span: Span,
    pub path: String,
    pub alias: Option<String>,
    pub contract: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub id: NodeId,
    pub span: Span,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(LetStmt),
    LetElse(LetElseStmt),
    Assign(AssignStmt),
    If(IfStmt),
    While(WhileStmt),
    For(ForStmt),
    Return(ReturnStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Emit(EmitStmt),
    Schedule(ScheduleStmt),
    Update(UpdateStmt),
    Settle(SettleStmt),
    Propose(ProposeStmt),
    Next(NextStmt),
    Require(RequireStmt),
    Match(MatchStmt),
    Expr(ExprStmt),
    OnceGuardPass(Span),
    Error(Span),
}

impl Stmt {
    pub fn span(&self) -> &Span {
        match self {
            Stmt::Let(s) => &s.span,
            Stmt::LetElse(s) => &s.span,
            Stmt::Assign(s) => &s.span,
            Stmt::If(s) => &s.span,
            Stmt::While(s) => &s.span,
            Stmt::For(s) => &s.span,
            Stmt::Return(s) => &s.span,
            Stmt::Break(s) => &s.span,
            Stmt::Continue(s) => &s.span,
            Stmt::Emit(s) => &s.span,
            Stmt::Schedule(s) => &s.span,
            Stmt::Update(s) => &s.span,
            Stmt::Settle(s) => &s.span,
            Stmt::Propose(s) => &s.span,
            Stmt::Next(s) => &s.span,
            Stmt::Require(s) => &s.span,
            Stmt::Match(s) => &s.span,
            Stmt::Expr(s) => &s.span,
            Stmt::OnceGuardPass(span) => span,
            Stmt::Error(span) => span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettleStmt {
    pub id: NodeId,
    pub span: Span,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ProposeStmt {
    pub id: NodeId,
    pub span: Span,
    pub intent_name: String,
    pub fields: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub struct NextStmt {
    pub id: NodeId,
    pub span: Span,
    pub entity: Expr,
    pub component_name: String,
    pub fields: Vec<(String, Expr)>,
}

#[derive(Debug, Clone)]
pub struct RequireStmt {
    pub id: NodeId,
    pub span: Span,
    pub condition: Expr,
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct ScheduleStmt {
    pub id: NodeId,
    pub span: Span,
    pub systems: Vec<String>,
    /// `schedule serial [ ... ]` — run the whole schedule one system at a
    /// time in topological order, no worker snapshots, no merge (dogfood
    /// feature seq 83: the per-call spelling of `--serial-schedule`).
    pub serial: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateStmt {
    pub id: NodeId,
    pub span: Span,
    pub entity_expr: Option<Expr>,
    pub comp_name: String,
    pub field_updates: Vec<FieldUpdate>,
}

/// One `name = expr` (index: None) or `name[i] = expr` (index: Some) entry
/// in an `update` block. Entries apply in written order, so
/// `{ vals = xs, vals[0] = 1 }` starts from `xs` and then patches slot 0.
#[derive(Debug, Clone)]
pub struct FieldUpdate {
    pub name: String,
    pub index: Option<Expr>,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct PhaseDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub systems: Vec<String>,
    /// `serial phase P [ ... ]` — members form a serial group: they never
    /// share a parallel batch with each other, in any schedule that runs
    /// them (dogfood feature seq 83: "these systems are ordered and I do
    /// not want them raced" made sayable).
    pub serial: bool,
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub id: NodeId,
    pub span: Span,
    /// One or more binding names (`let a` → one element; `let (a, b)` → two).
    pub names: Vec<String>,
    /// True when parsed as `let (...)` (including `let (x)`).
    pub tuple_destructure: bool,
    pub mutable: bool,
    pub recursive: bool,
    pub is_unique: bool,
    /// True for top-level `pub let NAME = ...` module constants. Only
    /// meaningful on declaration-position lets; always false inside bodies.
    pub is_pub: bool,
    pub type_annotation: Option<TypeExpr>,
    pub value: Expr,
}

/// `let Some { value: hp } = subject else { ... }` / `let Ok { value: x } = r else { ... }`
/// Lowered in the compiler to `let hp = match subject { Some { ... } => hp, None => ... }`.
#[derive(Debug, Clone)]
pub struct LetElseStmt {
    pub id: NodeId,
    pub span: Span,
    pub mutable: bool,
    pub type_annotation: Option<TypeExpr>,
    pub variant_name: String,
    pub bindings: Vec<String>,
    pub pattern_bindings: Vec<MatchBinding>,
    pub has_rest: bool,
    pub subject: Expr,
    pub else_block: Block,
}

impl LetElseStmt {
    pub fn primary_binding_name(&self) -> Option<String> {
        let bindings: Vec<MatchBinding> = if !self.pattern_bindings.is_empty() {
            self.pattern_bindings.clone()
        } else {
            self.bindings
                .iter()
                .map(|name| MatchBinding {
                    name: name.clone(),
                    path: vec![name.clone()],
                })
                .collect()
        };
        if bindings.len() == 1 {
            Some(bindings[0].name.clone())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub id: NodeId,
    pub span: Span,
    pub target: Expr,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub id: NodeId,
    pub span: Span,
    pub condition: Expr,
    pub then_block: Block,
    pub else_block: Option<Block>,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub id: NodeId,
    pub span: Span,
    pub condition: Expr,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub id: NodeId,
    pub span: Span,
    pub bindings: Vec<String>,
    pub destructure_bindings: Option<Vec<String>>,
    pub iterable: Expr,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub id: NodeId,
    pub span: Span,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct BreakStmt {
    pub id: NodeId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ContinueStmt {
    pub id: NodeId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EmitStmt {
    pub id: NodeId,
    pub span: Span,
    pub event_name: String,
    pub fields: Vec<(String, Expr)>,
    /// `emit E { .. } after N` — the event fires after N event-flush
    /// cycles (game ticks) instead of on the next flush.
    pub delay: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct MatchStmt {
    pub id: NodeId,
    pub span: Span,
    pub subject: Expr,
    pub cases: Vec<MatchCase>,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Literal(Expr),
    Variant {
        path: Vec<String>,
        bindings: Vec<String>,
        pattern_bindings: Vec<MatchBinding>,
        has_rest: bool,
        is_bare_variant: bool,
    },
    HasComponent {
        component: String,
        binding: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct MatchCase {
    pub id: NodeId,
    pub span: Span,
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct MatchBinding {
    pub name: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExprStmt {
    pub id: NodeId,
    pub span: Span,
    pub expr: Expr,
}

#[derive(Debug, Clone)]
pub enum FStringPart {
    Lit(String),
    Expr(Box<Expr>, Option<String>),
}

#[derive(Debug, Clone)]
pub enum Expr {
    IntLit(i64, Span),
    FloatLit(f64, Span),
    StrLit(String, Span),
    BoolLit(bool, Span),
    NilLit(Span),
    ListLit(Vec<Expr>, Span),
    MapLit(Vec<(Expr, Expr)>, Span),
    TupleLit(Vec<Expr>, Span),
    FStringExpr(Vec<FStringPart>, Span),
    Ident(String, Span),
    Binary(Box<Expr>, BinOp, Box<Expr>, Span),
    Unary(UnaryOp, Box<Expr>, Span),
    Pipe(Box<Expr>, Box<Expr>, Span),
    Call(Box<Expr>, Vec<Expr>, Span),
    Field(Box<Expr>, String, Span),
    Index(Box<Expr>, Box<Expr>, Span),
    ComponentExpr(String, Vec<(String, Expr)>, Option<Box<Expr>>, Span),
    StateRef(String, String, Span),
    /// Qualified `system::Name` or `system::a::b::Name` path (after `system::`, segments are idents).
    SystemRef(Vec<String>, Span),
    VariantExpr(String, String, Vec<(String, Expr)>, Span),
    MatchExpr(Box<MatchStmt>, Span),
    /// `if cond { a } else { b }` in expression position; else is
    /// mandatory and branches hold single expressions (chain via
    /// `else if`).
    IfExpr(Box<Expr>, Box<Expr>, Box<Expr>, Span),
    FnExpr(
        Vec<String>,
        Vec<bool>,
        Vec<Option<TypeExpr>>,
        Vec<Option<Vec<String>>>,
        Option<TypeExpr>,
        Block,
        Span,
    ),
    QueryExpr(QueryExprNode, Span),
    Await(Box<Expr>, Span),
    AsyncCall(Box<Expr>, Vec<Expr>, Span),
    Try(Box<Expr>, Span),
    Spread(Box<Expr>, Span),
    EntityLiteral(Option<Box<Expr>>, Vec<ComponentEntry>, Span),
    Error(Span),
}

impl Expr {
    pub fn span(&self) -> &Span {
        match self {
            Expr::IntLit(_, s) => s,
            Expr::FloatLit(_, s) => s,
            Expr::StrLit(_, s) => s,
            Expr::BoolLit(_, s) => s,
            Expr::NilLit(s) => s,
            Expr::ListLit(_, s) => s,
            Expr::MapLit(_, s) => s,
            Expr::FStringExpr(_, s) => s,
            Expr::Ident(_, s) => s,
            Expr::Binary(_, _, _, s) => s,
            Expr::Unary(_, _, s) => s,
            Expr::Pipe(_, _, s) => s,
            Expr::Call(_, _, s) => s,
            Expr::Field(_, _, s) => s,
            Expr::Index(_, _, s) => s,
            Expr::ComponentExpr(_, _, _, s) => s,
            Expr::StateRef(_, _, s) => s,
            Expr::SystemRef(_, s) => s,
            Expr::VariantExpr(_, _, _, s) => s,
            Expr::MatchExpr(_, s) => s,
            Expr::IfExpr(_, _, _, s) => s,
            Expr::FnExpr(_, _, _, _, _, _, s) => s,
            Expr::QueryExpr(_, s) => s,
            Expr::Await(_, s) => s,
            Expr::AsyncCall(_, _, s) => s,
            Expr::Try(_, s) => s,
            Expr::TupleLit(_, s) => s,
            Expr::Spread(_, s) => s,
            Expr::EntityLiteral(_, _, s) => s,
            Expr::Error(s) => s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Is,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Generic(String, Vec<TypeExpr>),
    Tuple(Vec<TypeExpr>),
    FnType(Vec<TypeExpr>, Box<TypeExpr>, FnTypePurity),
    Union(Vec<TypeExpr>),
}

/// Purity modifier written on a fn TYPE annotation: `pure fn(...) -> T`,
/// `readonly fn(...) -> T`, or a bare `fn(...) -> T` (`Default`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnTypePurity {
    Default,
    Pure,
    Readonly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Component,
    Struct,
}

#[derive(Debug, Clone)]
pub struct DataDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub kind: DataKind,
    /// `component X v2 { … }` — declared schema version, embedded per type
    /// in `save_world()` output and handed to `migrate X(old, from_version)`
    /// on load (dogfood feature seq 69 IDEA 03). 0 = undeclared; only
    /// meaningful for `DataKind::Component`.
    pub version: u32,
    pub fields: Vec<FieldDef>,
    pub indexed_fields: Vec<String>,
}

impl DataDecl {
    pub fn is_component(&self) -> bool {
        matches!(self.kind, DataKind::Component)
    }

    pub fn is_struct(&self) -> bool {
        matches!(self.kind, DataKind::Struct)
    }

    pub fn field_names(&self) -> Vec<String> {
        self.fields.iter().map(|field| field.name.clone()).collect()
    }
}

pub type ComponentDecl = DataDecl;
pub type StructDecl = DataDecl;
