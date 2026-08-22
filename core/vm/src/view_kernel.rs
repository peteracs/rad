//! Verified data-oriented execution plans for allocation-free materialized-view
//! traversal. Plans are emitted only after the ordinary checker has proved the
//! source callback's authority, ownership, and field types. They replace VM
//! dispatch, not language semantics.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewKernelArithmetic {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ViewKernelOperand {
    Field { component: String, field: String },
    Int(i64),
    Float(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewKernelWrite {
    pub component: String,
    pub field: String,
    pub left: ViewKernelOperand,
    pub operation: ViewKernelArithmetic,
    pub right: ViewKernelOperand,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewKernelPlan {
    pub view: String,
    pub writes: Vec<ViewKernelWrite>,
}
