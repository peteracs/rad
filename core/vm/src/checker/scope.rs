use super::*;
use crate::ast::*;
use crate::types::*;
use std::collections::HashMap;
impl Checker {
    pub(super) fn push_scope(&mut self) {
        let prev = self.scopes.last().unwrap();
        let new = Scope {
            bindings: HashMap::new(),
            in_system: prev.in_system.clone(),
            in_pipeline: prev.in_pipeline,
            in_async: prev.in_async,
            in_loop: prev.in_loop,
            settlement_depth: prev.settlement_depth,
            // Loop targets are lexical boundaries, not inherited flags.
            // Nested scopes find the nearest target by walking outward.
            loop_target_settlement_depth: None,
            effect_context: prev.effect_context.clone(),
            causal_context: prev.causal_context.clone(),
        };
        self.scopes.push(new);
    }
    /// Top-level `let` bindings live in the root scope, which is never popped; flush those here.
    pub(super) fn flush_unused_top_level_lets(&mut self) {
        let Some(scope) = self.scopes.first() else {
            return;
        };
        let pending: Vec<_> = scope
            .bindings
            .iter()
            .filter(|(n, b)| b.track_unused && !b.read && !n.starts_with('_'))
            .map(|(n, b)| (n.clone(), b.defined_at.clone()))
            .collect();
        for (name, span) in pending {
            self.warning(
                &span,
                format!("Unused variable '{}'", name),
                Some(
                    "If this is intentional, prefix the name with `_` or remove the binding"
                        .to_string(),
                ),
            );
        }
    }

    pub(super) fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            let to_warn: Vec<(Span, String)> = self
                .scopes
                .last()
                .unwrap()
                .bindings
                .iter()
                .filter(|(name, binding)| {
                    binding.track_unused && !binding.read && !name.starts_with('_')
                })
                .map(|(name, binding)| (binding.defined_at.clone(), name.clone()))
                .collect();
            for (span, name) in to_warn {
                self.warning(
                    &span,
                    format!("Unused variable '{}'", name),
                    Some(
                        "If this is intentional, prefix the name with `_` or remove the binding"
                            .to_string(),
                    ),
                );
            }
            self.scopes.pop();
        }
    }
    pub(super) fn define(
        &mut self,
        name: &str,
        ty: Ty,
        mutable: bool,
        span: Span,
        is_pub: bool,
        track_unused: bool,
    ) {
        if !name.starts_with('_') && ty != Ty::Any {
            let mut found_existing = None;
            for (depth, scope) in self.scopes.iter().enumerate().rev() {
                if let Some(binding) = scope.bindings.get(name) {
                    found_existing = Some((binding.clone(), depth));
                    break;
                }
            }

            if let Some((existing, existing_depth)) = found_existing {
                let current_depth = self.scopes.len() - 1;
                let is_local_shadow = existing_depth > 0;
                let is_same_scope_user_redef =
                    existing_depth == current_depth && existing.track_unused;
                if existing.ty == ty && (is_local_shadow || is_same_scope_user_redef) {
                    self.warning(
                        &span,
                        format!(
                            "Variable '{}' shadows an existing variable with the exact same type",
                            name
                        ),
                        Some("Rename this variable to avoid confusion".to_string()),
                    );
                }
            }

            // A non-function binding named like a builtin makes later calls
            // resolve to the binding, not the builtin (`range` the parameter
            // vs `range()` the iterator) — surface it at the definition.
            if !matches!(ty, Ty::Fn { .. }) && crate::builtins::is_builtin(name) {
                self.warning(
                    &span,
                    format!("'{}' shadows the builtin function '{}()'", name, name),
                    Some(format!(
                        "Calls to {}(...) in this scope will hit the {} binding, not the builtin — rename to avoid the trap",
                        name, name
                    )),
                );
            }
        }

        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(
                name.to_string(),
                Binding {
                    ty,
                    mutable,
                    is_unique: false,
                    defined_at: span,
                    is_pub,
                    track_unused,
                    read: false,
                },
            );
        }
    }
    pub(super) fn mark_var_read(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.bindings.get_mut(name) {
                binding.read = true;
                return;
            }
        }
    }
    pub(super) fn lookup(&self, name: &str) -> Option<Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.bindings.get(name) {
                return Some(binding.clone());
            }
        }
        None
    }

    pub(super) fn lookup_with_depth(&self, name: &str) -> Option<(Binding, usize)> {
        for (depth, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(binding) = scope.bindings.get(name) {
                return Some((binding.clone(), depth));
            }
        }
        None
    }

    pub(super) fn mark_binding_unique(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.bindings.get_mut(name) {
                binding.is_unique = true;
                return;
            }
        }
    }

    pub(super) fn binding_is_unique(&self, name: &str) -> bool {
        self.lookup(name).map(|b| b.is_unique).unwrap_or(false)
    }

    pub(super) fn binding_defined_in_current_pipeline(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if !scope.in_pipeline {
                break;
            }
            if scope.bindings.contains_key(name) {
                return true;
            }
        }
        false
    }

    /// Check if `name` is immutable in the current (innermost) scope but a
    /// mutable binding with the same name exists in an outer scope.  Returns
    /// `true` when the immutable binding shadows a mutable one — the typical
    /// case for pattern-destructured fields that collide with `let mut` vars.
    pub(super) fn is_shadow_of_outer_mutable(&self, name: &str) -> bool {
        let mut found_current = false;
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.bindings.get(name) {
                if !found_current {
                    if binding.mutable {
                        return false;
                    }
                    found_current = true;
                } else {
                    return binding.mutable;
                }
            }
        }
        false
    }
}
