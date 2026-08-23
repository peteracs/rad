// Lowering invocation expressions: direct and dynamic calls, async spawn,
// await, and the try postfix.

impl Compiler {
    pub(crate) fn compile_call_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::Call(callee, args, span) => {
                if let Expr::Ident(name, _) = callee.as_ref() {
                    let is_shadowed = self.has_lexical_binding(name);
                    if !is_shadowed
                        && name == "visit_view"
                        && self.try_compile_view_kernel(args, span)?
                    {
                        return Ok(());
                    }
                    if !is_shadowed && name == "has" && args.len() == 2 {
                        let component = match &args[1] {
                            Expr::Ident(component, _) => {
                                Some(self.resolve_canonical_name(component))
                            }
                            Expr::Field(owner, member, _)
                                if matches!(owner.as_ref(), Expr::Ident(_, _)) =>
                            {
                                let Expr::Ident(alias, _) = owner.as_ref() else {
                                    unreachable!()
                                };
                                self.resolve_alias_member(alias, member)
                            }
                            _ => None,
                        };
                        if let Some(component) = component {
                            self.compile_expr(&args[0])?;
                            let component_idx =
                                self.add_constant_gc(|gc| Value::from_string(gc, component));
                            self.emit_op(Op::EcsHas, span.line);
                            self.emit_u16(component_idx, span.line);
                            return Ok(());
                        }
                    }
                    if !is_shadowed && matches!(name.as_str(), "read_field" | "write_field") {
                        let expected = if name == "read_field" { 3 } else { 4 };
                        if args.len() == expected {
                            let component = match &args[1] {
                                Expr::Ident(component, _) => {
                                    Some(self.resolve_canonical_name(component))
                                }
                                Expr::Field(owner, member, _)
                                    if matches!(owner.as_ref(), Expr::Ident(_, _)) =>
                                {
                                    let Expr::Ident(alias, _) = owner.as_ref() else {
                                        unreachable!()
                                    };
                                    self.resolve_alias_member(alias, member)
                                }
                                _ => None,
                            };
                            let field = match &args[2] {
                                Expr::StrLit(field, _) => Some(field.clone()),
                                _ => None,
                            };
                            if let (Some(component), Some(field)) = (component, field) {
                                self.compile_expr(&args[0])?;
                                if name == "write_field" {
                                    self.compile_expr(&args[3])?;
                                }
                                let component_idx =
                                    self.add_constant_gc(|gc| Value::from_string(gc, component));
                                let field_idx =
                                    self.add_constant_gc(|gc| Value::from_string(gc, field));
                                self.emit_op(
                                    if name == "read_field" {
                                        Op::EcsReadField
                                    } else {
                                        Op::EcsWriteField
                                    },
                                    span.line,
                                );
                                self.emit_u16(component_idx, span.line);
                                self.emit_u16(field_idx, span.line);
                                return Ok(());
                            }
                        }
                    }
                }
                if let Expr::Ident(read_kind, _) = callee.as_ref() {
                    if matches!(read_kind.as_str(), "base" | "candidate") && args.len() == 2 {
                        if let Expr::Ident(component, _) = &args[1] {
                            self.compile_expr(&args[0])?;
                            let component = self.resolve_canonical_name(component);
                            let index =
                                self.add_constant_gc(|gc| Value::from_string(gc, component));
                            self.emit_op(
                                if read_kind == "base" {
                                    Op::ReadBaseComponent
                                } else {
                                    Op::ReadCandidateComponent
                                },
                                span.line,
                            );
                            self.emit_u16(index, span.line);
                            return Ok(());
                        }
                    }
                }
                if self.release && args.len() == 1 {
                    let mut is_debug_trace = false;
                    if let Expr::Ident(name, _) = callee.as_ref() {
                        let is_local = self.has_lexical_binding(name);
                        if !is_local {
                            if let Some(builtin) = Builtin::ALL.iter().find(|b| b.name() == name) {
                                if matches!(builtin, Builtin::DebugTrace) {
                                    is_debug_trace = true;
                                }
                            }
                        }
                    }
                    if is_debug_trace {
                        return self.compile_expr(&args[0]);
                    }
                }
                let resolved_system_name: Option<String> =
                    if let Expr::Field(obj, member, _) = callee.as_ref() {
                        if let Expr::Ident(alias, _) = obj.as_ref() {
                            self.resolve_alias_member(alias, member)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                let system_check_name: Option<&str> = if let Expr::Ident(name, _) = callee.as_ref()
                {
                    Some(name.as_str())
                } else {
                    resolved_system_name.as_deref()
                };
                if let Some(name) = system_check_name {
                    if self.is_system(name) {
                        if !args.is_empty() {
                            return Err(CompileError {
                                message: format!(
                                    "System '{}' takes no explicit arguments — its parameters come from the ECS world. Use `{}()` with no args.",
                                    name, name
                                ),
                                line: span.line,
                                col: span.col,
                            });
                        }
                        let name_idx =
                            self.add_constant_gc(|gc| Value::from_string(gc, name.to_string()));
                        self.emit_op(Op::RunSystem, span.line);
                        self.emit_u16(name_idx, span.line);
                        self.emit_constant(Value::NIL, span.line);
                        return Ok(());
                    }
                }
                let mut total_args = 0;
                let direct_builtin = if let Expr::Ident(name, _) = callee.as_ref() {
                    // Builtins begin life in the global slot table, but any
                    // user declaration with the same spelling owns normal
                    // lexical resolution. `predeclare_decl_metadata` records
                    // all top-level bindings before hoisted function bodies
                    // compile, so this remains source-order independent.
                    let is_shadowed = self.has_lexical_binding(name)
                        || self.global_mutability.contains_key(name);
                    (!is_shadowed)
                        .then(|| Builtin::from_name(name))
                        .flatten()
                } else {
                    None
                };
                for arg in args {
                    if let Expr::Spread(inner, s_span) = arg {
                        self.compile_expr(inner)?;
                        self.emit_op(Op::Unpack, span.line);
                        let len = self.spread_lengths.get(s_span).copied().unwrap_or(1);
                        total_args += len;
                    } else {
                        self.compile_expr(arg)?;
                        total_args += 1;
                    }
                }
                if let Some(builtin) = direct_builtin {
                    self.emit_op(Op::CallBuiltin, span.line);
                    self.emit_u16(builtin as u16, span.line);
                    self.emit_byte(total_args as u8, span.line);
                } else {
                    self.compile_expr(callee)?;
                    self.emit_op(Op::Call, span.line);
                    self.emit_byte(total_args as u8, span.line);
                }
            }
            Expr::Await(inner, _) => {
                self.compile_expr(inner)?;
                self.emit_op(Op::Await, inner.span().line);
            }
            Expr::Try(inner, span) => {
                self.compile_expr(inner)?;
                self.emit_op(Op::Try, span.line);
            }
            Expr::AsyncCall(callee, args, span) => {
                let mut total_args = 0;
                for arg in args {
                    if let Expr::Spread(inner, s_span) = arg {
                        self.compile_expr(inner)?;
                        self.emit_op(Op::Unpack, span.line);
                        let len = self.spread_lengths.get(s_span).copied().unwrap_or(1);
                        total_args += len;
                    } else {
                        self.compile_expr(arg)?;
                        total_args += 1;
                    }
                }
                self.compile_expr(callee)?;
                self.emit_op(Op::AsyncCall, span.line);
                self.emit_byte(total_args as u8, span.line);
            }
            other => unreachable!(
                "compile_call_expr received {other:?}, which its dispatcher never routes here"
            ),
        }
        Ok(())
    }
}
