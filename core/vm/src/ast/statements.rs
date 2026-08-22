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
    Transaction(TransactionStmt),
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
            Stmt::Transaction(s) => &s.span,
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
pub struct NativeTypeDecl {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub is_pub: bool,
    pub repr: crate::native_types::NativeScalarKind,
    pub flavor: crate::native_types::NativeTypeFlavor,
    /// Ordered source declarations. Opaque/scalar declarations leave this
    /// empty; enum values and bit masks are represented as exact u64 bits.
    pub members: Vec<(String, u64)>,
}

/// A stable atomic world-state boundary. Preconditions observe the base
/// snapshot, body writes remain private until commit, postconditions observe
/// the complete candidate, and `post_commit` executes only after adoption.
#[derive(Debug, Clone)]
pub struct TransactionStmt {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
    pub changes_only: Vec<String>,
    pub requires: Vec<Expr>,
    pub body: Block,
    pub ensures: Vec<Expr>,
    pub post_commit: Option<Block>,
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
    /// `serial phase P { ... }` — members form a serial group: they never
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
    pub delivery: EventDelivery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventDelivery {
    /// Strictly double-buffered delivery on the next event flush.
    Next,
    /// Immediate handler execution in the current call stack.
    Sync,
    /// Delivery when the named lifecycle phase is entered.
    Phase(String),
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
