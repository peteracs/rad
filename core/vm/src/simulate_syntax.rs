//! Static shape of `simulate(fork, systems, ticks)` at compile time.
//!
//! The parser treats this as a normal call so it fits the expression grammar and tooling. The type
//! checker and other analyses still enforce a **macro- or keyword-like** restriction on
//! `systems`: a **list literal of `system::…` references** (phase C). String literals are rejected
//! with a migration diagnostic.
//!
//! **Phase E:** `schedule [system::A, …]` uses the same `system::` path syntax as expressions.
//! **Phase F:** VM `schedule` bytecode stores `system` reference constants (not strings), matching
//! `simulate`'s runtime list.
//! **Phase G:** Self-hosted C emit (`emit_c.rad`) lowers `system::…` exprs to strings for
//! `rad_dispatch_system`; the Rad LSP resolves `system::` paths for hover and go-to-definition.
//!
//! Internal `Builtin::Simulate` remains the canonical runtime spelling;
//! [`callee_name`] matches the built-in's canonical name.

use crate::ast::Expr;
use crate::value::Builtin;

/// Index of the `systems` argument in `simulate(fork, systems, ticks)`.
pub const SYSTEMS_ARG_INDEX: usize = 1;

/// Minimum arity so `systems` is present for static classification.
#[inline]
pub const fn min_args_for_systems_check() -> usize {
    SYSTEMS_ARG_INDEX + 1
}

#[inline]
pub fn callee_name() -> &'static str {
    Builtin::Simulate.name()
}

/// Builtin name and enough arguments to carry a `systems` expression.
#[inline]
pub fn is_named_call(callee: &str, argc: usize) -> bool {
    is_simulate_family(callee) && argc >= min_args_for_systems_check()
}

/// All builtins that take a `systems` schedule at [`SYSTEMS_ARG_INDEX`] and
/// share `simulate`'s static-schedule and purity rules. `simulate_many` takes
/// a list of forks at arg 0 but its schedule is still at arg 1, and
/// `simulate_seeded` is a single exact-seed rollout — for all of them the
/// same static classification and const-folding apply.
#[inline]
pub fn is_simulate_family(callee: &str) -> bool {
    callee == callee_name()
        || callee == Builtin::SimulatePar.name()
        || callee == Builtin::SimulateMany.name()
        || callee == Builtin::SimulateSeeded.name()
}

/// Direct `Expr::Ident` call to `simulate` with enough arguments.
#[inline]
pub fn is_expr_call(callee: &Expr, args: &[Expr]) -> bool {
    matches!(callee, Expr::Ident(n, _) if is_named_call(n, args.len()))
}

/// Build the qualified name used by the checker and compiler's internal
/// canonical-name resolution: the first segment may be a module alias;
/// further segments join with `.` (e.g. `a::b::c` → `a.b.c`).
#[inline]
pub fn system_ref_qualified_string(path: &[String]) -> String {
    match path.len() {
        0 => String::new(),
        1 => path[0].clone(),
        _ => format!("{}.{}", path[0], path[1..].join(".")),
    }
}

#[inline]
pub fn is_typed_schedule_element(expr: &Expr) -> bool {
    matches!(expr, Expr::SystemRef(_, _))
}

#[inline]
fn is_string_only_schedule_element(expr: &Expr) -> bool {
    matches!(expr, Expr::StrLit(_, _))
}

/// How the `systems` argument is spelled in source.
#[derive(Clone, Copy, Debug)]
pub enum SystemsListForm<'a> {
    /// `[system::A, …]` — the only accepted static shape for `simulate`.
    StaticSchedule(&'a [Expr]),
    /// `["A", …]` — legacy; rejected (phase C).
    StringLiteralSchedule(&'a [Expr]),
    /// Both strings and `system::…` in the same list.
    MixedLiteralSchedule(&'a [Expr]),
    /// A list literal containing a non–string/non–system-ref expression.
    NonStaticListLiteral,
    /// Not a list literal (variable, call, etc.).
    NotListLiteral,
}

/// Compare static typed schedules without requiring `Expr: Eq`.
fn static_schedule_contents_eq(a: &[Expr], b: &[Expr]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| match (x, y) {
            (Expr::SystemRef(p1, _), Expr::SystemRef(p2, _)) => p1 == p2,
            _ => false,
        })
}

impl PartialEq for SystemsListForm<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SystemsListForm::StaticSchedule(a), SystemsListForm::StaticSchedule(b)) => {
                static_schedule_contents_eq(a, b)
            }
            (
                SystemsListForm::StringLiteralSchedule(a),
                SystemsListForm::StringLiteralSchedule(b),
            ) => {
                a.len() == b.len()
                    && a.iter().zip(b.iter()).all(|(x, y)| match (x, y) {
                        (Expr::StrLit(s1, _), Expr::StrLit(s2, _)) => s1 == s2,
                        _ => false,
                    })
            }
            (
                SystemsListForm::MixedLiteralSchedule(a),
                SystemsListForm::MixedLiteralSchedule(b),
            ) => std::ptr::eq(*a, *b),
            (SystemsListForm::NonStaticListLiteral, SystemsListForm::NonStaticListLiteral) => true,
            (SystemsListForm::NotListLiteral, SystemsListForm::NotListLiteral) => true,
            _ => false,
        }
    }
}

/// Classify the second argument to `simulate` for compile-time analysis.
#[inline]
pub fn classify_systems_argument(expr: &Expr) -> SystemsListForm<'_> {
    match expr {
        Expr::ListLit(items, _) => {
            if items.is_empty() {
                return SystemsListForm::StaticSchedule(items.as_slice());
            }
            let all_typed = items.iter().all(is_typed_schedule_element);
            let all_strings = items.iter().all(is_string_only_schedule_element);
            let only_strings_or_refs = items
                .iter()
                .all(|e| is_typed_schedule_element(e) || is_string_only_schedule_element(e));
            if !only_strings_or_refs {
                SystemsListForm::NonStaticListLiteral
            } else if all_typed {
                SystemsListForm::StaticSchedule(items.as_slice())
            } else if all_strings {
                SystemsListForm::StringLiteralSchedule(items.as_slice())
            } else {
                SystemsListForm::MixedLiteralSchedule(items.as_slice())
            }
        }
        _ => SystemsListForm::NotListLiteral,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;

    fn sp() -> Span {
        Span {
            line: 1,
            col: 0,
            file: None,
        }
    }

    #[test]
    fn static_typed_schedule_classified() {
        let e = Expr::ListLit(
            vec![
                Expr::SystemRef(vec!["A".to_string()], sp()),
                Expr::SystemRef(vec!["B".to_string()], sp()),
            ],
            sp(),
        );
        assert!(matches!(
            classify_systems_argument(&e),
            SystemsListForm::StaticSchedule(items) if items.len() == 2
        ));
    }

    #[test]
    fn string_only_schedule_is_legacy_variant() {
        let e = Expr::ListLit(
            vec![
                Expr::StrLit("A".to_string(), sp()),
                Expr::StrLit("B".to_string(), sp()),
            ],
            sp(),
        );
        assert!(matches!(
            classify_systems_argument(&e),
            SystemsListForm::StringLiteralSchedule(_)
        ));
    }

    #[test]
    fn non_string_element_is_non_static() {
        let e = Expr::ListLit(
            vec![
                Expr::StrLit("A".to_string(), sp()),
                Expr::Ident("x".to_string(), sp()),
            ],
            sp(),
        );
        assert_eq!(
            classify_systems_argument(&e),
            SystemsListForm::NonStaticListLiteral
        );
    }

    #[test]
    fn expr_call_detection() {
        let callee = Expr::Ident(callee_name().to_string(), sp());
        let args = vec![
            Expr::NilLit(sp()),
            Expr::ListLit(vec![], sp()),
            Expr::IntLit(1, sp()),
        ];
        assert!(is_expr_call(&callee, &args));
        assert!(!is_expr_call(
            &Expr::Ident("other".to_string(), sp()),
            &args
        ));
    }

    #[test]
    fn mixed_string_and_system_ref_is_mixed_form() {
        let e = Expr::ListLit(
            vec![
                Expr::StrLit("A".to_string(), sp()),
                Expr::SystemRef(vec!["B".to_string()], sp()),
            ],
            sp(),
        );
        assert!(matches!(
            classify_systems_argument(&e),
            SystemsListForm::MixedLiteralSchedule(_)
        ));
    }

    #[test]
    fn system_ref_qualified_joins_path() {
        assert_eq!(
            system_ref_qualified_string(&["m".to_string(), "S".to_string()]),
            "m.S"
        );
        assert_eq!(system_ref_qualified_string(&["S".to_string()]), "S");
    }
}
