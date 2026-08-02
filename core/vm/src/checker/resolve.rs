use super::*;
use crate::ast::*;
use crate::types::*;

impl Checker {
    pub(super) fn type_expr_or_any(&mut self, type_expr: &TypeExpr) -> Ty {
        let span = Span::default();
        match self.resolve_type_expr_inner(type_expr, &span) {
            Some(ty) => ty,
            None => {
                self.warnings.push(crate::checker::TypeWarning {
                    line: span.line,
                    col: span.col,
                    file: span.file,
                    message: format!(
                        "Unknown type '{}' in annotation; defaulting to any",
                        format_type_expr(type_expr)
                    ),
                    hint: Some("Check that this type is declared before use".to_string()),
                });
                Ty::Any
            }
        }
    }
    pub(super) fn resolve_type_expr(&mut self, type_expr: &TypeExpr, span: &Span) -> Ty {
        if let Some(ty) = self.resolve_type_expr_inner(type_expr, span) {
            return ty;
        }
        self.error(
            span,
            format!("Unknown type annotation '{}'", format_type_expr(type_expr)),
            None,
        );
        Ty::Any
    }
    pub(super) fn resolve_type_expr_inner(
        &mut self,
        type_expr: &TypeExpr,
        span: &Span,
    ) -> Option<Ty> {
        match type_expr {
            TypeExpr::Named(name) => self.lookup_type_name(name, span),
            TypeExpr::Generic(name, args) => {
                let name = &self.resolve_canonical_name(name);
                let resolved_args: Vec<Ty> = args
                    .iter()
                    .filter_map(|a| self.resolve_type_expr_inner(a, span))
                    .collect();
                if let Some(sum_def) = self.sum_types.get(name).cloned() {
                    if !sum_def.is_pub && is_cross_file(sum_def.file_id, span.file) {
                        self.error(
                            span,
                            format!("Type '{}' is private", name),
                            Some(format!("Add `pub` to the declaration of '{}'", name)),
                        );
                    }
                    if sum_def.type_params.is_empty() {
                        Some(Ty::SumType(name.clone()))
                    } else {
                        Some(Ty::App(name.clone(), resolved_args))
                    }
                } else if let Some(alias) = self.type_aliases.get(name).cloned() {
                    if !alias.is_pub && is_cross_file(alias.file_id, span.file) {
                        self.error(
                            span,
                            format!("Type alias '{}' is private", name),
                            Some(format!("Add `pub` to the declaration of '{}'", name)),
                        );
                    }
                    if alias.type_params.len() != resolved_args.len() {
                        return Some(Ty::Any);
                    }
                    let mapping: std::collections::HashMap<String, Ty> = alias
                        .type_params
                        .iter()
                        .cloned()
                        .zip(resolved_args)
                        .collect();
                    Some(self.substitute_type_params(&alias.ty, &mapping))
                } else if name == "list" && resolved_args.len() == 1 {
                    Some(Ty::List(Box::new(resolved_args[0].clone())))
                } else if name == "map" && resolved_args.len() == 2 {
                    Some(Ty::Map(
                        Box::new(resolved_args[0].clone()),
                        Box::new(resolved_args[1].clone()),
                    ))
                } else if name == "map" && resolved_args.len() == 1 {
                    Some(Ty::Map(
                        Box::new(Ty::Str),
                        Box::new(resolved_args[0].clone()),
                    ))
                } else {
                    Some(Ty::App(name.clone(), resolved_args))
                }
            }
            TypeExpr::FnType(params, ret, purity) => {
                let param_tys: Vec<Ty> = params
                    .iter()
                    .map(|p| self.resolve_type_expr_inner(p, span).unwrap_or(Ty::Any))
                    .collect();
                let ret_ty = self.resolve_type_expr_inner(ret, span).unwrap_or(Ty::Any);
                Some(Ty::Fn {
                    params: param_tys,
                    ret: Box::new(ret_ty),
                    purity: match purity {
                        FnTypePurity::Pure => FnPurity::Pure,
                        FnTypePurity::Readonly => FnPurity::Readonly,
                        // A bare `fn(...)` annotation promises nothing.
                        FnTypePurity::Default => FnPurity::Impure,
                    },
                })
            }
            TypeExpr::Union(variants) => {
                let mut merged = Ty::Void;
                for v in variants {
                    let resolved = self.resolve_type_expr_inner(v, span).unwrap_or(Ty::Any);
                    merged = Ty::union(&merged, &resolved);
                }
                Some(merged)
            }
            TypeExpr::Tuple(types) => {
                let resolved: Vec<Ty> = types
                    .iter()
                    .map(|t| self.resolve_type_expr_inner(t, span).unwrap_or(Ty::Any))
                    .collect();
                Some(Ty::Tuple(resolved))
            }
        }
    }
    pub(super) fn lookup_type_name(&mut self, name: &str, span: &Span) -> Option<Ty> {
        let name = &self.resolve_canonical_name(name);
        match name.as_str() {
            "int" => Some(Ty::Int),
            "float" => Some(Ty::Float),
            "str" => Some(Ty::Str),
            "bool" => Some(Ty::Bool),
            "nil" => Some(Ty::Nil),
            "any" => Some(Ty::Any),
            "void" => Some(Ty::Void),
            "entity" => Some(Ty::EntityId),
            // Forks are first-class values that cross pub fn boundaries in
            // multi-module programs; they need a surface type name.
            "world_fork" => Some(Ty::WorldFork),
            "bitset" => Some(Ty::BitSet),
            "system" => Some(Ty::SystemRef),
            "list" => Some(Ty::List(Box::new(Ty::Any))),
            "map" => Some(Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any))),
            other => {
                if self
                    .type_param_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains(other))
                {
                    return Some(Ty::App(other.to_string(), vec![]));
                }
                if let Some(alias) = self.type_aliases.get(other).cloned() {
                    if !alias.is_pub && is_cross_file(alias.file_id, span.file) {
                        self.error(
                            span,
                            format!("Type alias '{}' is private", other),
                            Some(format!("Add `pub` to the declaration of '{}'", other)),
                        );
                    }
                    if alias.type_params.is_empty() {
                        return Some(alias.ty.clone());
                    }
                    return Some(Ty::App(other.to_string(), vec![]));
                }
                if let Some(st) = self.structs.get(other) {
                    if !st.is_pub && is_cross_file(st.file_id, span.file) {
                        self.error(
                            span,
                            format!("Struct '{}' is private", other),
                            Some(format!("Add `pub` to the declaration of '{}'", other)),
                        );
                    }
                    Some(Ty::Struct(other.to_string()))
                } else if let Some(comp) = self.components.get(other) {
                    if !comp.is_pub && is_cross_file(comp.file_id, span.file) {
                        self.error(
                            span,
                            format!("Component '{}' is private", other),
                            Some(format!("Add `pub` to the declaration of '{}'", other)),
                        );
                    }
                    Some(Ty::Component(other.to_string()))
                } else if let Some(res) = self.resources.get(other) {
                    if !res.is_pub && is_cross_file(res.file_id, span.file) {
                        self.error(
                            span,
                            format!("Resource '{}' is private", other),
                            Some(format!("Add `pub` to the declaration of '{}'", other)),
                        );
                    }
                    // Resources are "structurally identical to components"
                    // (spec §3.1.1); a resource value is typed Ty::Component
                    // everywhere else (component-expr, get_resource). Resolve
                    // the name the same way so a resource is spellable as a
                    // fn parameter/return annotation, not just a system param.
                    Some(Ty::Component(other.to_string()))
                } else if let Some(sm) = self.state_machines.get(other) {
                    if !sm.is_pub && is_cross_file(sm.file_id, span.file) {
                        self.error(
                            span,
                            format!("State machine '{}' is private", other),
                            Some(format!("Add `pub` to the declaration of '{}'", other)),
                        );
                    }
                    Some(Ty::State(other.to_string()))
                } else if let Some(sum_def) = self.sum_types.get(other) {
                    if !sum_def.is_pub && is_cross_file(sum_def.file_id, span.file) {
                        self.error(
                            span,
                            format!("Type '{}' is private", other),
                            Some(format!("Add `pub` to the declaration of '{}'", other)),
                        );
                    }
                    Some(Ty::SumType(other.to_string()))
                } else {
                    None
                }
            }
        }
    }
    pub(super) fn infer_literal_type(&self, expr: &Expr) -> Ty {
        match expr {
            Expr::IntLit(_, _) => Ty::Int,
            Expr::FloatLit(_, _) => Ty::Float,
            Expr::StrLit(_, _) => Ty::Str,
            Expr::BoolLit(_, _) => Ty::Bool,
            Expr::NilLit(_) => Ty::Nil,
            Expr::ListLit(elems, _) => {
                if elems.is_empty() {
                    Ty::List(Box::new(Ty::Any))
                } else {
                    let first = self.infer_literal_type(&elems[0]);
                    let mut elem_ty = first;
                    for e in &elems[1..] {
                        let et = self.infer_literal_type(e);
                        if elem_ty != Ty::Any && et != Ty::Any && !elem_ty.assignable_from(&et) {
                            if et.assignable_from(&elem_ty) {
                                elem_ty = et;
                            } else {
                                elem_ty = Ty::Any;
                            }
                        }
                    }
                    Ty::List(Box::new(elem_ty))
                }
            }
            Expr::MapLit(entries, _) => {
                if entries.is_empty() {
                    Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any))
                } else {
                    let key_ty = self.infer_literal_type(&entries[0].0);
                    let val_ty = self.infer_literal_type(&entries[0].1);
                    Ty::Map(Box::new(key_ty), Box::new(val_ty))
                }
            }
            Expr::ComponentExpr(name, _, _, _) => {
                let name = self.resolve_canonical_name(name);
                if self.components.contains_key(&name) {
                    Ty::Component(name)
                } else if self.structs.contains_key(&name) {
                    Ty::Struct(name)
                } else {
                    Ty::Any
                }
            }
            Expr::VariantExpr(type_name, _, _, _) => {
                let type_name = self.resolve_canonical_name(type_name);
                if let Some(sum_type) = self.sum_types.get(&type_name) {
                    if sum_type.type_params.is_empty() {
                        Ty::SumType(type_name)
                    } else {
                        // We can't infer type arguments from just the literal here easily
                        Ty::App(type_name, vec![Ty::Any; sum_type.type_params.len()])
                    }
                } else {
                    Ty::Any
                }
            }
            Expr::Ident(name, _) => {
                let name = self.resolve_canonical_name(name);
                if let Some(sum_type) = self.sum_types.get(&name) {
                    if sum_type.type_params.is_empty() {
                        Ty::SumType(name)
                    } else {
                        Ty::App(name, vec![Ty::Any; sum_type.type_params.len()])
                    }
                } else if self.structs.contains_key(&name) {
                    Ty::Struct(name)
                } else if self.components.contains_key(&name) {
                    Ty::Component(name)
                } else {
                    Ty::Any
                }
            }
            Expr::SystemRef(path, _) => {
                let q = crate::simulate_syntax::system_ref_qualified_string(path);
                let resolved = self.resolve_canonical_name(&q);
                if self.systems.contains_key(&resolved) {
                    Ty::SystemRef
                } else {
                    Ty::Any
                }
            }
            _ => Ty::Any,
        }
    }
}
