impl Checker {
    /// Check a value against an expected type while allowing unresolved
    /// generic constructors to learn from that boundary.
    ///
    /// `None` is inferred as `Option<?T>` because it carries no payload. A
    /// plain `assignable_from` check therefore rejected the canonical
    /// annotation `let value: Option<u32> = None`, and the same split logic in
    /// assignment and record construction could reject `Some(...)` later.
    /// Probe unification on a cloned substitution so a failed compatibility
    /// check cannot partially poison subsequent inference; commit only when
    /// the fully resolved value is assignable to the expected type.
    fn accepts_inferred_value(&mut self, expected: &Ty, actual: &Ty) -> bool {
        let expected = self.resolve_ty(expected);
        let actual = self.resolve_ty(actual);
        if expected.assignable_from(&actual) {
            return true;
        }

        let mut candidate = self.subst.clone();
        if candidate.unify(&expected, &actual).is_err() {
            return false;
        }
        let resolved_expected = candidate.resolve(&expected);
        let resolved_actual = candidate.resolve(&actual);
        if !resolved_expected.assignable_from(&resolved_actual) {
            return false;
        }
        self.subst = candidate;
        true
    }
}
