//! Declarative plans for transactionally maintained materialized views.
//!
//! View predicates intentionally are not arbitrary guest callbacks. A compact,
//! closed plan can be evaluated while a world patch commits without re-entering
//! the VM, performing IO, allocating guest objects, or hiding dependencies.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewComparison {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl ViewComparison {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }

    pub fn reversed(self) -> Self {
        match self {
            Self::Eq => Self::Eq,
            Self::Ne => Self::Ne,
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewPredicateValue {
    Int(i64),
    Float(u64),
    Str(String),
    Bool(bool),
}

impl std::fmt::Display for ViewPredicateValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Float(bits) => write!(formatter, "{}", f64::from_bits(*bits)),
            Self::Str(value) => write!(formatter, "{value:?}"),
            Self::Bool(value) => write!(formatter, "{value}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewPredicateClause {
    pub component: String,
    pub field: String,
    pub comparison: ViewComparison,
    pub expected: ViewPredicateValue,
}

impl std::fmt::Display for ViewPredicateClause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}.{} {} {}",
            self.component,
            self.field,
            self.comparison.symbol(),
            self.expected
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterializedViewPredicate {
    /// Conjunction in source order. The first failed clause is the operational
    /// explanation returned by `why_not_in_view`.
    pub clauses: Vec<ViewPredicateClause>,
}
