// Lowering the expressions that build world and sum-type values: components,
// state references, variants, and entity literals.

impl Compiler {
    pub(crate) fn compile_construction_expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match expr {
            Expr::ComponentExpr(name, fields, rest, span) => {
                let name = &self.resolve_canonical_name(name);
                let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, name.clone()));
                // Declared defaults fill omitted fields; resources keep
                // theirs in a separate table.
                let defaults = self
                    .component_types
                    .get(name)
                    .or_else(|| self.resource_types.get(name))
                    .cloned()
                    .unwrap_or_default();

                if let Some(slot_order) = self.component_field_order(name) {
                    let field_count = slot_order.len();
                    for slot_name in &slot_order {
                        if let Some((_, fexpr)) = fields.iter().find(|(n, _)| n == slot_name) {
                            self.compile_component_field(name, slot_name, fexpr, span)?;
                        } else if let Some(base) = rest {
                            self.compile_expr(base)?;
                            let field_idx = self
                                .add_constant_gc(|gc| Value::from_string(gc, slot_name.clone()));
                            self.emit_op(Op::GetField, span.line);
                            self.emit_u16(field_idx, span.line);
                        } else if let Some((_, _, default_expr)) =
                            defaults.iter().find(|(n, _, _)| n == slot_name)
                        {
                            self.compile_expr(default_expr)?;
                        } else {
                            self.emit_constant(Value::NIL, span.line);
                        }
                    }
                    self.emit_op(Op::MakeCompSlot, span.line);
                    self.emit_u16(type_idx, span.line);
                    self.emit_u16(field_count as u16, span.line);
                } else {
                    let mut all_fields: Vec<(String, Option<&Expr>)> = Vec::new();
                    for (fname, _, fexpr) in &defaults {
                        all_fields.push((fname.clone(), Some(fexpr)));
                    }
                    for (fname, fexpr) in fields {
                        if let Some(existing) = all_fields.iter_mut().find(|(n, _)| n == fname) {
                            existing.1 = Some(fexpr);
                        } else {
                            all_fields.push((fname.clone(), Some(fexpr)));
                        }
                    }

                    let explicit_field_names: std::collections::HashSet<&str> =
                        fields.iter().map(|(n, _)| n.as_str()).collect();

                    let field_count = all_fields.len();
                    for (fname, fexpr) in &all_fields {
                        self.emit_constant_gc(span.line, |gc| {
                            Value::from_string(gc, fname.clone())
                        });
                        if explicit_field_names.contains(fname.as_str()) {
                            if let Some(expr) = fexpr {
                                self.compile_component_field(name, fname, expr, span)?;
                            } else {
                                self.emit_constant(Value::NIL, span.line);
                            }
                        } else if let Some(base) = rest {
                            self.compile_expr(base)?;
                            let field_idx =
                                self.add_constant_gc(|gc| Value::from_string(gc, fname.clone()));
                            self.emit_op(Op::GetField, span.line);
                            self.emit_u16(field_idx, span.line);
                        } else if let Some(expr) = fexpr {
                            self.compile_expr(expr)?;
                        } else {
                            self.emit_constant(Value::NIL, span.line);
                        }
                    }
                    self.emit_op(Op::MakeComp, span.line);
                    self.emit_u16(type_idx, span.line);
                    self.emit_u16(field_count as u16, span.line);
                }
            }
            Expr::StateRef(machine, state, span) => {
                let machine = &self.resolve_canonical_name(machine);
                if let Some(native) = self.native_types.get(machine).cloned() {
                    let bits = native.member_bits(state).ok_or(CompileError {
                        message: format!("Unknown native member '{}::{}'", machine, state),
                        line: span.line,
                        col: span.col,
                    })?;
                    self.emit_constant_gc(span.line, |gc| {
                        Value::from_native_scalar(
                            gc,
                            crate::native_types::NativeScalarValue {
                                type_name: native.name,
                                repr: native.repr,
                                flavor: native.flavor,
                                bits,
                            },
                        )
                    });
                } else if self
                    .variant_shorthand
                    .contains(&(machine.clone(), state.clone()))
                {
                    let type_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, machine.clone()));
                    let variant_idx =
                        self.add_constant_gc(|gc| Value::from_string(gc, state.clone()));
                    self.emit_op(Op::MakeVariant, span.line);
                    self.emit_u16(type_idx, span.line);
                    self.emit_u16(variant_idx, span.line);
                    self.emit_u16(0, span.line);
                } else {
                    let m_idx = self.add_constant_gc(|gc| Value::from_string(gc, machine.clone()));
                    let s_idx = self.add_constant_gc(|gc| Value::from_string(gc, state.clone()));
                    self.emit_op(Op::MakeState, span.line);
                    self.emit_u16(m_idx, span.line);
                    self.emit_u16(s_idx, span.line);
                }
            }
            Expr::VariantExpr(type_name, variant, fields, span) => {
                let type_name = &self.resolve_canonical_name(type_name);
                if let Some(native) = self.native_types.get(type_name).cloned() {
                    if !fields.is_empty() {
                        return Err(CompileError {
                            message: format!(
                                "Native member '{}::{}' cannot have fields",
                                type_name, variant
                            ),
                            line: span.line,
                            col: span.col,
                        });
                    }
                    let bits = native.member_bits(variant).ok_or(CompileError {
                        message: format!("Unknown native member '{}::{}'", type_name, variant),
                        line: span.line,
                        col: span.col,
                    })?;
                    self.emit_constant_gc(span.line, |gc| {
                        Value::from_native_scalar(
                            gc,
                            crate::native_types::NativeScalarValue {
                                type_name: native.name,
                                repr: native.repr,
                                flavor: native.flavor,
                                bits,
                            },
                        )
                    });
                    return Ok(());
                }
                let type_idx = self.add_constant_gc(|gc| Value::from_string(gc, type_name.clone()));
                let variant_idx =
                    self.add_constant_gc(|gc| Value::from_string(gc, variant.clone()));
                let field_count = fields.len();
                for (fname, fexpr) in fields {
                    self.emit_constant_gc(span.line, |gc| Value::from_string(gc, fname.clone()));
                    self.compile_expr(fexpr)?;
                }
                self.emit_op(Op::MakeVariant, span.line);
                self.emit_u16(type_idx, span.line);
                self.emit_u16(variant_idx, span.line);
                self.emit_u16(field_count as u16, span.line);
            }
            Expr::EntityLiteral(name, components, span) => {
                if let Some(name_expr) = name {
                    self.compile_expr(name_expr)?;
                }
                self.compile_component_inits(components, span.line)?;
                let comp_count = components.len() as u8;
                self.emit_op(Op::EcsSpawn, span.line);
                self.emit_byte(comp_count, span.line);
                if name.is_some() {
                    self.emit_byte(1, span.line);
                    self.emit_u16(0, span.line);
                } else {
                    self.emit_byte(0, span.line);
                    let name_idx = self.add_constant_gc(|gc| Value::from_string(gc, String::new()));
                    self.emit_u16(name_idx, span.line);
                }
            }
            other => unreachable!(
                "compile_construction_expr received {other:?}, which its dispatcher never routes here"
            ),
        }
        Ok(())
    }
}
