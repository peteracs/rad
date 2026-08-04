impl Checker {

fn check_typed_collection_builtin(
        &mut self,
        name: &str,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
    ) -> Option<Ty> {
        match name {
            "get" if arg_tys.len() == 2 => {
                if let Some(Expr::Ident(sn, _)) = arg_exprs.get(1) {
                    if self.structs.contains_key(sn.as_str()) {
                        self.error(
                            arg_exprs.get(1).unwrap().span(),
                            format!("Cannot use `get()` with struct type '{}'; only components can be used with ECS operations", sn),
                            Some(format!("Declare '{}' with `component` instead of `struct` to use it with ECS", sn)),
                        );
                    }
                }
                let comp_name = self.resolve_component_name(arg_exprs.get(1), arg_tys.get(1));
                if let Some(cn) = comp_name {
                    if self.components.contains_key(&cn) {
                        return Some(Ty::App("Option".to_string(), vec![Ty::Component(cn)]));
                    }
                }
                Some(Ty::App("Option".to_string(), vec![Ty::Any]))
            }
            "require" if arg_tys.len() == 2 => {
                let comp_name = self.resolve_component_name(arg_exprs.get(1), arg_tys.get(1));
                if let Some(cn) = comp_name {
                    if self.components.contains_key(&cn) {
                        return Some(Ty::Component(cn));
                    }
                }
                Some(Ty::Any)
            }
            "require_all" if arg_tys.len() >= 2 => Some(Ty::List(Box::new(Ty::Any))),
            // set_at: (T, K, V) -> T for T = list or map. The key must fit
            // the collection (int for lists, the key type for maps) and the
            // result keeps the collection's own type.
            "set_at" if arg_tys.len() == 3 => match &arg_tys[0] {
                Ty::List(_) => {
                    if arg_tys[1] != Ty::Int && arg_tys[1] != Ty::Any {
                        self.error(
                            arg_exprs
                                .get(1)
                                .map(|e| e.span())
                                .unwrap_or(&Span::default()),
                            format!("set_at() list index must be int, got {}", arg_tys[1]),
                            None,
                        );
                    }
                    Some(arg_tys[0].clone())
                }
                Ty::Map(key_ty, _) => {
                    if **key_ty != Ty::Any
                        && arg_tys[1] != Ty::Any
                        && self.subst.unify(key_ty, &arg_tys[1]).is_err()
                    {
                        self.error(
                            arg_exprs
                                .get(1)
                                .map(|e| e.span())
                                .unwrap_or(&Span::default()),
                            format!("set_at() map key expects {}, got {}", key_ty, arg_tys[1]),
                            None,
                        );
                    }
                    Some(arg_tys[0].clone())
                }
                Ty::Any => Some(Ty::Any),
                other => {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!("set_at() expects a list or map, got {}", other),
                        None,
                    );
                    Some(Ty::Any)
                }
            },
            "keys" if arg_tys.len() == 1 => match &arg_tys[0] {
                Ty::Component(_) | Ty::Struct(_) => Some(Ty::List(Box::new(Ty::Str))),
                Ty::Map(key_ty, _) => Some(Ty::List(key_ty.clone())),
                Ty::Any => Some(Ty::List(Box::new(Ty::Any))),
                _ => None,
            },
            "values" if arg_tys.len() == 1 => match &arg_tys[0] {
                Ty::Component(_) | Ty::Struct(_) => Some(Ty::List(Box::new(Ty::Any))),
                Ty::Map(_, val_ty) => Some(Ty::List(val_ty.clone())),
                Ty::Any => Some(Ty::List(Box::new(Ty::Any))),
                _ => None,
            },
            "entries" if arg_tys.len() == 1 => {
                match &arg_tys[0] {
                    Ty::Component(_) | Ty::Struct(_) => {
                        Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))))
                    }
                    Ty::Map(_key_ty, _val_ty) => {
                        // The inner list has type Any because it contains both K and V
                        Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))))
                    }
                    Ty::Any => Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any))))),
                    _ => None,
                }
            }
            "set" if arg_tys.len() == 2 => {
                if let Some(Ty::Struct(sn)) = arg_tys.get(1) {
                    self.error(
                        arg_exprs
                            .get(1)
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Cannot use `set()` with struct type '{}'; \
                             only components can be used with ECS operations",
                            sn
                        ),
                        Some(format!(
                            "Declare '{}' with `component` instead of `struct` to use it with ECS",
                            sn
                        )),
                    );
                }
                if let Some(Ty::Component(comp_name)) = arg_tys.get(1) {
                    if let Some(comp_type) = self.components.get(comp_name).cloned() {
                        if let Some(Expr::ComponentExpr(_, fields, _, _)) = arg_exprs.get(1) {
                            for (field_name, field_expr) in fields {
                                // The field_expr is already checked in check_expr when arg_tys was populated
                                // We just need to check if it's assignable
                                let actual_ty = self.check_expr(field_expr);
                                if let Some(expected_ty) = comp_type.field_type(field_name) {
                                    if !expected_ty.assignable_from(&actual_ty)
                                        && actual_ty != Ty::Any
                                    {
                                        self.error(
                                            field_expr.span(),
                                            format!(
                                                "Type error in '{}.{}': expected {}, got {}",
                                                comp_name, field_name, expected_ty, actual_ty
                                            ),
                                            None,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                Some(Ty::Nil)
            }
            "transition" if arg_tys.len() == 2 => {
                let state_ty = self.resolve_ty(&arg_tys[0]);
                Some(Ty::App("Result".to_string(), vec![state_ty, Ty::Str]))
            }
            "contains" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::Bool);
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let expected_elem = match &subject_ty {
                    Ty::List(inner) => Some(inner.as_ref().clone()),
                    Ty::Str => Some(Ty::Str),
                    Ty::Map(key, _) => Some(key.as_ref().clone()),
                    Ty::Any => None,
                    _ => {
                        self.error(
                            arg_exprs[0].span(),
                            format!(
                                "Argument 1 to 'contains' expects list, str, or map, got {}",
                                subject_ty
                            ),
                            None,
                        );
                        None
                    }
                };
                if let Some(expected) = expected_elem {
                    if !expected.assignable_from(&arg_tys[1]) && arg_tys[1] != Ty::Any {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'contains' expects {}, got {}",
                                expected, arg_tys[1]
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::Bool)
            }
            "sort" | "reverse" => {
                if !self.check_builtin_arg_count(name, 1, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                match subject_ty {
                    Ty::List(inner) => Some(Ty::List(inner)),
                    Ty::Str => Some(Ty::Str),
                    _ => Some(Ty::Any),
                }
            }
            "slice" => {
                if arg_tys.len() < 2 || arg_tys.len() > 3 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'slice' expects 2 to 3 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                    return Some(Ty::Any);
                }
                for i in 1..arg_tys.len() {
                    if !Ty::Int.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to 'slice' expects int, got {}",
                                i + 1,
                                arg_tys[i]
                            ),
                            None,
                        );
                    }
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                match subject_ty {
                    Ty::List(inner) => Some(Ty::List(inner)),
                    Ty::Str => Some(Ty::Str),
                    _ => Some(Ty::Any),
                }
            }
            "append" | "extend" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let mut valid = true;
                for i in 0..2 {
                    let ty = self.resolve_ty(&arg_tys[i]);
                    if !matches!(ty, Ty::List(_) | Ty::Str | Ty::Any) {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to '{}' expects list or str, got {}",
                                i + 1,
                                name,
                                ty
                            ),
                            None,
                        );
                        valid = false;
                    }
                }
                if !valid {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let t1 = match self.resolve_ty(&arg_tys[0]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                let t2 = match self.resolve_ty(&arg_tys[1]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                match subject_ty {
                    Ty::Str => Some(Ty::Str),
                    _ => {
                        if t1 == t2 && t1 != Ty::Any {
                            Some(Ty::List(Box::new(t1)))
                        } else {
                            Some(Ty::List(Box::new(Ty::Any)))
                        }
                    }
                }
            }
            "flat_map" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                let mut ret_ty = Ty::Any;
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    if let Ty::List(inner) = ret {
                        ret_ty = *inner;
                    } else if ret != Ty::Any {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'flat_map' callback must return a list, got {}",
                                ret
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::List(Box::new(ret_ty)))
            }
            "map" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                let mut ret_ty = Ty::Any;
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    ret_ty = ret;
                }
                Some(Ty::List(Box::new(ret_ty)))
            }
            "filter" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    if ret != Ty::Bool && ret != Ty::Any {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'filter' callback must return bool, got {}",
                                ret
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::List(Box::new(elem_ty)))
            }
            "reduce" => {
                if !self.check_builtin_arg_count(name, 3, arg_tys, arg_exprs) {
                    return Some(Ty::Any);
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let acc_ty = self.resolve_ty(&arg_tys[1]);
                let func_ty = self.resolve_ty(&arg_tys[2]);
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 2, arg_exprs[2].span())
                {
                    if params.len() >= 2 {
                        if !params[0].assignable_from(&acc_ty) && acc_ty != Ty::Any {
                            self.error(
                                arg_exprs[2].span(),
                                format!(
                                    "Argument 3 to 'reduce' expects fn({}, ...), got fn({}, ...)",
                                    acc_ty, params[0]
                                ),
                                None,
                            );
                        }
                        if !params[1].assignable_from(&elem_ty) && elem_ty != Ty::Any {
                            self.error(
                                arg_exprs[2].span(),
                                format!(
                                    "Argument 3 to 'reduce' expects fn(..., {}), got fn(..., {})",
                                    elem_ty, params[1]
                                ),
                                None,
                            );
                        }
                    }
                    if !acc_ty.assignable_from(&ret) && ret != Ty::Any && acc_ty != Ty::Any {
                        self.error(
                            arg_exprs[2].span(),
                            format!(
                                "Argument 3 to 'reduce' callback must return {}, got {}",
                                acc_ty, ret
                            ),
                            None,
                        );
                    }
                }
                Some(acc_ty)
            }
            "group_by" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::Map(
                        Box::new(Ty::Str),
                        Box::new(Ty::List(Box::new(Ty::Any))),
                    ));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                if let Some((params, ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                    if !ret.is_valid_map_key() {
                        self.error(
                            arg_exprs[1].span(),
                            format!(
                                "Argument 2 to 'group_by' callback must return a valid map key (str, int, bool, entity, or tuples of those), got {}",
                                ret
                            ),
                            None,
                        );
                        return Some(Ty::Map(
                            Box::new(Ty::Any),
                            Box::new(Ty::List(Box::new(elem_ty))),
                        ));
                    }
                    // the result map is keyed by whatever the key fn returns
                    return Some(Ty::Map(
                        Box::new(ret),
                        Box::new(Ty::List(Box::new(elem_ty))),
                    ));
                }
                Some(Ty::Map(
                    Box::new(Ty::Any),
                    Box::new(Ty::List(Box::new(elem_ty))),
                ))
            }
            "sort_by" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::Any)));
                }
                let subject_ty = self.resolve_ty(&arg_tys[0]);
                let elem_ty =
                    self.extract_sequence_element_type(name, &subject_ty, arg_exprs[0].span());
                let func_ty = self.resolve_ty(&arg_tys[1]);
                if let Some((params, _ret)) =
                    self.require_callback_arg(name, &func_ty, 1, arg_exprs[1].span())
                {
                    self.validate_callback_param_type(
                        name,
                        &params,
                        &elem_ty,
                        1,
                        arg_exprs[1].span(),
                    );
                }
                match subject_ty {
                    Ty::List(inner) => Some(Ty::List(inner)),
                    Ty::Str => Some(Ty::Str),
                    _ => Some(Ty::Any),
                }
            }
            "zip" => {
                if !self.check_builtin_arg_count(name, 2, arg_tys, arg_exprs) {
                    return Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))));
                }
                let mut valid = true;
                for i in 0..2 {
                    let ty = self.resolve_ty(&arg_tys[i]);
                    if !matches!(ty, Ty::List(_) | Ty::Str | Ty::Any) {
                        self.error(
                            arg_exprs[i].span(),
                            format!(
                                "Argument {} to 'zip' expects list or str, got {}",
                                i + 1,
                                ty
                            ),
                            None,
                        );
                        valid = false;
                    }
                }
                if !valid {
                    return Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))));
                }
                let t1 = match self.resolve_ty(&arg_tys[0]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                let t2 = match self.resolve_ty(&arg_tys[1]) {
                    Ty::List(inner) => *inner,
                    Ty::Str => Ty::Str,
                    _ => Ty::Any,
                };
                if t1 == t2 && t1 != Ty::Any {
                    Some(Ty::List(Box::new(Ty::List(Box::new(t1)))))
                } else {
                    Some(Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))))
                }
            }
            _ => None,
        }
    }

fn check_typed_query_builtin(
        &mut self,
        name: &str,
        arg_tys: &[Ty],
        arg_exprs: &[&Expr],
    ) -> Option<Ty> {
        match name {
            "unwrap" if arg_tys.len() == 1 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(args[0].clone())
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => Some(Ty::Any),
                    Ty::Any => Some(Ty::Any),
                    _ => None, // Fall through to normal type checking to report error
                }
            }
            "expect" if arg_tys.len() == 2 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(args[0].clone())
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => Some(Ty::Any),
                    Ty::Any => Some(Ty::Any),
                    _ => None, // Fall through to normal type checking to report error
                }
            }
            "unwrap_or" if arg_tys.len() == 2 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::Any => Some(self.resolve_ty(&arg_tys[1])),
                    _ => None,
                }
            }
            "map_or" if arg_tys.len() == 3 => {
                let container_ty = self.resolve_ty(&arg_tys[0]);
                match container_ty {
                    Ty::App(name, args) if name == "Option" && args.len() == 1 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::App(name, args) if name == "Result" && args.len() == 2 => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::SumType(name) if name == "Option" || name == "Result" => {
                        Some(self.resolve_ty(&arg_tys[1]))
                    }
                    Ty::Any => Some(self.resolve_ty(&arg_tys[1])),
                    _ => None,
                }
            }
            "query_where" => {
                if arg_tys.len() < 2 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'query_where' expects at least 2 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                } else {
                    for i in 0..arg_tys.len() - 1 {
                        if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "Argument {} to 'query_where' expects str, got {}",
                                    i + 1,
                                    arg_tys[i]
                                ),
                                None,
                            );
                        }
                    }
                    let pred_ty = &arg_tys[arg_tys.len() - 1];
                    let pred_expr = arg_exprs[arg_tys.len() - 1];
                    let expected_pred = Ty::Fn {
                        params: vec![Ty::EntityId],
                        ret: Box::new(Ty::Bool),
                        purity: FnPurity::Pure,
                    };
                    if !expected_pred.assignable_from(pred_ty) && *pred_ty != Ty::Any {
                        // Separate an EFFECT mismatch (right shape, but the
                        // predicate has side effects) from a genuine shape
                        // mismatch. The former otherwise renders as "expects
                        // pure fn(entity) -> bool, got fn(entity) -> bool" —
                        // two identical-looking types differing by one
                        // easily-missed word, with no clue why. Name the
                        // offending call, reusing the pipeline breach finder.
                        let impure_shape = Ty::Fn {
                            params: vec![Ty::EntityId],
                            ret: Box::new(Ty::Bool),
                            purity: FnPurity::Impure,
                        };
                        if impure_shape.assignable_from(pred_ty) {
                            // World READS are fine: query_where snapshots
                            // the matching entity list before the predicate
                            // runs, so `get`/`res`/`has`/`readonly fn` in
                            // the body observe a stable world. The contract
                            // is read-only, not pure — filtering by
                            // component values is the builtin's whole
                            // purpose. Only writes, IO, events, and
                            // unverifiable calls remain breaches.
                            // A value whose TYPE already vouches (a pure or
                            // readonly fn type — explicit annotation, vouched
                            // closure, named pure/readonly fn, or a promoted
                            // callback param) needs no body walk:
                            // assignability guarantees only conforming values
                            // inhabit it.
                            let type_vouches = matches!(
                                pred_ty,
                                Ty::Fn {
                                    purity: FnPurity::Pure | FnPurity::Readonly,
                                    ..
                                }
                            );
                            let breach = if type_vouches {
                                None
                            } else {
                                match pred_expr {
                                    Expr::FnExpr(_, _, _, _, _, body, _) => {
                                        self.find_block_readonly_breach(body)
                                    }
                                    Expr::Ident(name, _) => {
                                        let resolved = self.resolve_canonical_name(name);
                                        match self
                                            .functions
                                            .get(&resolved)
                                            .or_else(|| self.functions.get(name.as_str()))
                                        {
                                            Some(sig)
                                                if sig.is_pure
                                                    || Self::effect_set_is_read_only(
                                                        &sig.effects,
                                                    ) =>
                                            {
                                                None
                                            }
                                            _ => Some(format!(
                                                "'{}' is not a `pure fn` or `readonly fn`",
                                                name
                                            )),
                                        }
                                    }
                                    _ => Some(
                                        "cannot be verified as read-only (only inline closures and named fns are analyzable)"
                                            .to_string(),
                                    ),
                                }
                            };
                            if let Some(reason) = breach {
                                let hint = format!(
                                    "the predicate {} — query_where runs it during iteration, so world reads are allowed but writes, IO, and events are not",
                                    reason
                                );
                                self.error(
                                    pred_expr.span(),
                                    format!(
                                        "Argument {} to 'query_where' must be a read-only predicate, but the one given has side effects",
                                        arg_tys.len()
                                    ),
                                    Some(hint),
                                );
                            }
                        } else {
                            self.error(
                                pred_expr.span(),
                                format!(
                                    "Argument {} to 'query_where' expects {}, got {}",
                                    arg_tys.len(),
                                    expected_pred,
                                    pred_ty
                                ),
                                None,
                            );
                        }
                    }
                }
                Some(Ty::List(Box::new(Ty::EntityId)))
            }
            "query_map" => {
                let mut ret_ty = Ty::Any;
                if arg_tys.len() < 2 {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        format!(
                            "Function 'query_map' expects at least 2 argument(s), got {}",
                            arg_tys.len()
                        ),
                        None,
                    );
                } else {
                    for i in 0..arg_tys.len() - 1 {
                        if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "Argument {} to 'query_map' expects str, got {}",
                                    i + 1,
                                    arg_tys[i]
                                ),
                                None,
                            );
                        }
                    }
                    let map_fn_ty = &arg_tys[arg_tys.len() - 1];
                    let map_expr = arg_exprs[arg_tys.len() - 1];
                    if let Ty::Fn { ret, purity, .. } = map_fn_ty {
                        ret_ty = *ret.clone();
                        // Same read-only contract as query_where's predicate
                        // (the mapper runs during iteration over a
                        // snapshotted entity list): world reads and
                        // `readonly fn` calls are fine, writes/IO/events are
                        // not. A Pure or Readonly TYPE already vouches;
                        // only impure-typed mappers need the body walk.
                        if matches!(purity, FnPurity::Impure) {
                            let breach = match map_expr {
                                Expr::FnExpr(_, _, _, _, _, body, _) => {
                                    self.find_block_readonly_breach(body)
                                }
                                Expr::Ident(name, _) => {
                                    let resolved = self.resolve_canonical_name(name);
                                    match self
                                        .functions
                                        .get(&resolved)
                                        .or_else(|| self.functions.get(name.as_str()))
                                    {
                                        Some(sig)
                                            if sig.is_pure
                                                || Self::effect_set_is_read_only(
                                                    &sig.effects,
                                                ) =>
                                        {
                                            None
                                        }
                                        _ => Some(format!(
                                            "'{}' is not a `pure fn` or `readonly fn`",
                                            name
                                        )),
                                    }
                                }
                                _ => Some(
                                    "cannot be verified as read-only (only inline closures and named fns are analyzable)"
                                        .to_string(),
                                ),
                            };
                            if let Some(reason) = breach {
                                let hint = format!(
                                    "the mapper {} — query_map runs it during iteration, so world reads are allowed but writes, IO, and events are not",
                                    reason
                                );
                                self.error(
                                    map_expr.span(),
                                    format!(
                                        "Argument {} to 'query_map' must be a read-only mapper, but the one given has side effects",
                                        arg_tys.len()
                                    ),
                                    Some(hint),
                                );
                            }
                        }
                    } else if *map_fn_ty != Ty::Any {
                        self.error(
                            arg_exprs[arg_tys.len() - 1].span(),
                            format!(
                                "Argument {} to 'query_map' expects function, got {}",
                                arg_tys.len(),
                                map_fn_ty
                            ),
                            None,
                        );
                    }
                }
                Some(Ty::List(Box::new(ret_ty)))
            }
            "query_count" => {
                if arg_tys.is_empty() {
                    self.error(
                        arg_exprs
                            .first()
                            .map(|e| e.span())
                            .unwrap_or(&Span::default()),
                        "Function 'query_count' expects at least 1 argument(s), got 0".to_string(),
                        None,
                    );
                } else {
                    for i in 0..arg_tys.len() {
                        if !Ty::Str.assignable_from(&arg_tys[i]) && arg_tys[i] != Ty::Any {
                            self.error(
                                arg_exprs[i].span(),
                                format!(
                                    "Argument {} to 'query_count' expects str, got {}",
                                    i + 1,
                                    arg_tys[i]
                                ),
                                None,
                            );
                        }
                    }
                }
                Some(Ty::Int)
            }
            _ => None,
        }
    }}