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
    pub ordered_indexed_fields: Vec<String>,
    pub ownership: OwnershipDecl,
    /// Native ABI layout is opt-in. `packed` is only legal with `repr(C)`.
    pub repr_c: bool,
    pub packed: bool,
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
