impl Parser {
    pub(super) fn parse_materialized_view_decl(
        &mut self,
    ) -> Result<MaterializedViewDecl, ParseError> {
        let span = self.span();
        self.advance(); // materialized
        let view = self.expect_ident_text()?;
        debug_assert_eq!(view, "view");
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LBrace)?;
        let mut dependencies = Vec::new();
        let mut key = None;
        let mut predicate = crate::materialized_view::MaterializedViewPredicate::default();
        while !self.check(TokenType::RBrace) {
            if self.check_ident_text("depends") {
                self.advance();
                self.expect(TokenType::LBracket)?;
                while !self.check(TokenType::RBracket) {
                    dependencies.push(self.expect_ident_text()?);
                    if self.check(TokenType::Comma) {
                        self.advance();
                    } else if !self.check(TokenType::RBracket) {
                        return Err(ParseError {
                            message: "Expected `,` or `]` in view dependency list".to_string(),
                            line: self.peek().line,
                            col: self.peek().col,
                        });
                    }
                }
                self.expect(TokenType::RBracket)?;
            } else if self.check_ident_text("key") {
                self.advance();
                let component = self.expect_ident_text()?;
                self.expect(TokenType::Dot)?;
                let field = self.expect_ident_text()?;
                if key.replace((component, field)).is_some() {
                    return Err(ParseError {
                        message: format!("materialized view `{name}` declares more than one key"),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            } else if self.check_ident_text("where") {
                self.advance();
                if !predicate.clauses.is_empty() {
                    return Err(ParseError {
                        message: format!(
                            "materialized view `{name}` declares more than one `where` predicate"
                        ),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
                let expression = self.parse_expr()?;
                Self::lower_materialized_view_predicate(&expression, &mut predicate.clauses)?;
            } else {
                return Err(ParseError {
                    message: "Expected `depends [Component, ...]`, `key Component.field`, or `where Component.field <op> literal` in materialized view".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        dependencies.sort();
        dependencies.dedup();
        if dependencies.is_empty() {
            return Err(ParseError {
                message: format!("materialized view `{name}` requires at least one dependency"),
                line: span.line,
                col: span.col,
            });
        }
        if let Some((component, _)) = &key {
            if !dependencies.contains(component) {
                dependencies.push(component.clone());
                dependencies.sort();
            }
        }
        for clause in &predicate.clauses {
            if !dependencies.contains(&clause.component) {
                return Err(ParseError {
                    message: format!(
                        "materialized view `{name}` predicate reads `{}` but it is absent from `depends`; add `{}` so revision and maintenance invalidation stay exact",
                        clause.component, clause.component
                    ),
                    line: span.line,
                    col: span.col,
                });
            }
        }
        Ok(MaterializedViewDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            dependencies,
            key,
            predicate,
        })
    }

    fn lower_materialized_view_predicate(
        expression: &Expr,
        clauses: &mut Vec<crate::materialized_view::ViewPredicateClause>,
    ) -> Result<(), ParseError> {
        use crate::materialized_view::{
            ViewComparison, ViewPredicateClause, ViewPredicateValue,
        };

        if let Expr::Binary(left, BinOp::And, right, _) = expression {
            Self::lower_materialized_view_predicate(left, clauses)?;
            Self::lower_materialized_view_predicate(right, clauses)?;
            return Ok(());
        }

        let Expr::Binary(left, operator, right, span) = expression else {
            return Err(ParseError {
                message: "A materialized-view predicate must be a conjunction of field-to-literal comparisons".to_string(),
                line: expression.span().line,
                col: expression.span().col,
            });
        };
        let comparison = match operator {
            BinOp::Eq | BinOp::Is => ViewComparison::Eq,
            BinOp::Ne => ViewComparison::Ne,
            BinOp::Lt => ViewComparison::Lt,
            BinOp::Le => ViewComparison::Le,
            BinOp::Gt => ViewComparison::Gt,
            BinOp::Ge => ViewComparison::Ge,
            _ => {
                return Err(ParseError {
                    message: "Materialized-view predicates support only ==, !=, <, <=, >, and >= comparisons".to_string(),
                    line: span.line,
                    col: span.col,
                });
            }
        };

        fn field(expression: &Expr) -> Option<(&str, &str)> {
            let Expr::Field(base, name, _) = expression else {
                return None;
            };
            let Expr::Ident(component, _) = base.as_ref() else {
                return None;
            };
            Some((component, name))
        }

        fn literal(expression: &Expr) -> Option<ViewPredicateValue> {
            match expression {
                Expr::IntLit(value, _) => Some(ViewPredicateValue::Int(*value)),
                Expr::FloatLit(value, _) => Some(ViewPredicateValue::Float(value.to_bits())),
                Expr::StrLit(value, _) => Some(ViewPredicateValue::Str(value.clone())),
                Expr::BoolLit(value, _) => Some(ViewPredicateValue::Bool(*value)),
                Expr::Unary(UnaryOp::Neg, inner, _) => match inner.as_ref() {
                    Expr::IntLit(value, _) => Some(ViewPredicateValue::Int(-*value)),
                    Expr::FloatLit(value, _) => {
                        Some(ViewPredicateValue::Float((-*value).to_bits()))
                    }
                    _ => None,
                },
                _ => None,
            }
        }

        let (component, name, comparison, expected) =
            if let (Some((component, name)), Some(expected)) = (field(left), literal(right)) {
                (component, name, comparison, expected)
            } else if let (Some(expected), Some((component, name))) = (literal(left), field(right)) {
                (component, name, comparison.reversed(), expected)
            } else {
                return Err(ParseError {
                    message: "Each materialized-view clause must compare `Component.field` with an int, float, bool, or string literal".to_string(),
                    line: span.line,
                    col: span.col,
                });
            };
        clauses.push(ViewPredicateClause {
            component: component.to_string(),
            field: name.to_string(),
            comparison,
            expected,
        });
        Ok(())
    }

    fn parse_native_repr(&mut self) -> Result<crate::native_types::NativeScalarKind, ParseError> {
        let token = self.peek().clone();
        let name = self.expect_ident_text()?;
        crate::native_types::NativeScalarKind::parse(&name).ok_or_else(|| ParseError {
            message: format!(
                "'{}' is not a native scalar representation; expected u8/u16/u32/u64, i8/i16/i32/i64, f32, or f64",
                name
            ),
            line: token.line,
            col: token.col,
        })
    }

    pub(super) fn parse_opaque_type_decl(&mut self) -> Result<NativeTypeDecl, ParseError> {
        let span = self.span();
        self.advance(); // opaque
        self.expect(TokenType::Type)?;
        let name = self.expect_ident_text()?;
        self.expect(TokenType::Assign)?;
        let repr = self.parse_native_repr()?;
        Ok(NativeTypeDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            repr,
            flavor: crate::native_types::NativeTypeFlavor::Opaque,
            members: Vec::new(),
        })
    }

    pub(super) fn parse_native_members_decl(
        &mut self,
        flavor: crate::native_types::NativeTypeFlavor,
    ) -> Result<NativeTypeDecl, ParseError> {
        let span = self.span();
        self.advance(); // enum / bitflags
        let name = self.expect_ident_text()?;
        self.expect(TokenType::Colon)?;
        let repr = self.parse_native_repr()?;
        if repr.is_float() {
            return Err(ParseError {
                message: format!("{} '{}' requires an integer representation", if flavor == crate::native_types::NativeTypeFlavor::Enum { "enum" } else { "bitflags" }, name),
                line: span.line,
                col: span.col,
            });
        }
        self.expect(TokenType::LBrace)?;
        let mut members = Vec::new();
        while !self.check(TokenType::RBrace) {
            let member_span = self.span();
            let member = self.expect_ident_text()?;
            self.expect(TokenType::Assign)?;
            let negative = self.check(TokenType::Minus);
            if negative {
                self.advance();
            }
            let token = self.expect(TokenType::Int)?;
            let magnitude = self.token_int_value(&token, "native discriminant")?;
            let bits = if negative {
                (magnitude.checked_neg().ok_or_else(|| ParseError {
                    message: "native discriminant underflow".to_string(),
                    line: member_span.line,
                    col: member_span.col,
                })?) as u64
            } else {
                magnitude as u64
            };
            members.push((member, bits));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(NativeTypeDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            repr,
            flavor,
            members,
        })
    }

    pub(super) fn parse_repr_struct_decl(&mut self) -> Result<DataDecl, ParseError> {
        let leading_packed = self.check_ident_text("packed");
        if leading_packed {
            self.advance();
        }
        if !self.check_ident_text("repr") {
            return Err(ParseError {
                message: "`packed` requires `repr(C)`".to_string(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        self.advance();
        self.expect(TokenType::LParen)?;
        let convention = self.expect_ident_text()?;
        if convention != "C" {
            return Err(ParseError {
                message: format!("unsupported representation `repr({convention})`; RAD supports only repr(C)"),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        self.expect(TokenType::RParen)?;
        let trailing_packed = self.check_ident_text("packed");
        if trailing_packed {
            self.advance();
        }
        let mut decl = self.parse_data_decl(DataKind::Struct, false, true)?;
        decl.repr_c = true;
        decl.packed = leading_packed || trailing_packed;
        Ok(decl)
    }

fn parse_use(&mut self) -> Result<UseStmt, ParseError> {
        let span = self.span();
        self.expect(TokenType::Use)?;
        let path = self.expect_string_text()?;
        let alias = if self.check(TokenType::As) {
            self.advance();
            Some(self.expect_ident_text()?)
        } else {
            None
        };
        let contract = if self.check(TokenType::Colon) {
            self.advance();
            Some(self.expect_ident_text()?)
        } else {
            None
        };
        Ok(UseStmt {
            id: self.next_id(),
            span,
            path,
            alias,
            contract,
            module_identity: None,
        })
    }

    /// Optional schema-version tag between a component/resource name and its
    /// `{`: an identifier of exactly `v<digits>` (dogfood feature seq 69
    /// IDEA 03). Nothing else may appear in that position today, so the
    /// sniff cannot collide with user code.
    fn try_parse_schema_version(&mut self) -> u32 {
        if self.check(TokenType::Ident) {
            if let Some(text) = self.peek().value.as_str() {
                if let Some(digits) = text.strip_prefix('v') {
                    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(v) = digits.parse::<u32>() {
                            self.advance();
                            return v;
                        }
                    }
                }
            }
        }
        0
    }

    fn parse_ownership_decl(&mut self) -> Result<OwnershipDecl, ParseError> {
        if !self.check_ident_text("owned") {
            return Ok(OwnershipDecl::default());
        }
        self.advance();
        let transferred_to = if self.check_ident_text("transfer") {
            self.advance();
            if !self.check_ident_text("to") {
                return Err(ParseError {
                    message: "Expected `to` after `owned transfer`".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            self.advance();
            Some(self.expect_ident_text()?)
        } else {
            None
        };
        let mut coowner_modules = Vec::new();
        if self.check_ident_text("with") {
            self.advance();
            self.expect(TokenType::LBracket)?;
            while !self.check(TokenType::RBracket) {
                coowner_modules.push(self.expect_ident_text()?);
                if self.check(TokenType::Comma) {
                    self.advance();
                } else if !self.check(TokenType::RBracket) {
                    return Err(ParseError {
                        message: "Expected `,` or `]` in owned co-owner list".to_string(),
                        line: self.peek().line,
                        col: self.peek().col,
                    });
                }
            }
            self.expect(TokenType::RBracket)?;
        }
        Ok(OwnershipDecl {
            owned: true,
            coowner_modules,
            transferred_to,
        })
    }

    fn parse_data_decl(
        &mut self,
        kind: DataKind,
        allow_indexed: bool,
        require_named_field_types: bool,
    ) -> Result<DataDecl, ParseError> {
        let span = self.span();
        match kind {
            DataKind::Component => self.expect(TokenType::Component)?,
            DataKind::Struct => self.expect(TokenType::Struct)?,
        };
        let name = self.expect_ident_text()?;
        // `component X v2 { … }` — struct versions are meaningless for
        // persistence, so only components take the tag.
        let version = if matches!(kind, DataKind::Component) {
            self.try_parse_schema_version()
        } else {
            0
        };
        let ownership = if matches!(kind, DataKind::Component) {
            self.parse_ownership_decl()?
        } else {
            OwnershipDecl::default()
        };
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        let mut indexed_fields = Vec::new();
        let mut ordered_indexed_fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let is_ordered = allow_indexed
                && self.check_ident_text("ordered")
                && self.peek_at(1).ty == TokenType::Indexed;
            if is_ordered {
                self.advance();
            }
            // `indexed` is only the marker when a field name follows it —
            // `indexed: int = 0` is a field literally named "indexed".
            let is_indexed = allow_indexed
                && self.check(TokenType::Indexed)
                && self.peek_at(1).ty != TokenType::Colon;
            if is_indexed {
                self.advance();
            }
            let is_owned = self.check_ident_text("owned")
                && self.peek_at(1).ty != TokenType::Colon;
            if is_owned {
                self.advance();
            }
            let fname = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let (type_ann, fval, required) = if self.is_component_type_annotation() {
                let ty = self.parse_type()?;
                self.expect(TokenType::Assign)?;
                let expr = self.parse_expr()?;
                (Some(ty), expr, false)
            } else if let Some(ty) = self.try_annotation_only_field(require_named_field_types) {
                // `source: entity` — required at every construction
                let placeholder = Expr::NilLit(self.span());
                (Some(ty), placeholder, true)
            } else {
                (None, self.parse_expr()?, false)
            };
            let field_name = fname.clone();
            fields.push(FieldDef {
                name: fname,
                type_annotation: type_ann,
                default_value: fval,
                is_indexed,
                is_owned,
                required,
            });
            if is_indexed {
                indexed_fields.push(field_name);
                if is_ordered {
                    ordered_indexed_fields.push(fields.last().unwrap().name.clone());
                }
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(DataDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            kind,
            version,
            fields,
            indexed_fields,
            ordered_indexed_fields,
            ownership,
            repr_c: false,
            packed: false,
        })
    }

    fn parse_resource_decl(&mut self) -> Result<ResourceDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Resource)?;
        let name = self.expect_ident_text()?;
        let version = self.try_parse_schema_version();
        let ownership = self.parse_ownership_decl()?;
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let is_owned = self.check_ident_text("owned")
                && self.peek_at(1).ty != TokenType::Colon;
            if is_owned {
                self.advance();
            }
            let fname = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let (type_ann, fval, required) = if self.is_component_type_annotation() {
                let ty = self.parse_type()?;
                self.expect(TokenType::Assign)?;
                let expr = self.parse_expr()?;
                (Some(ty), expr, false)
            } else if let Some(ty) = self.try_annotation_only_field(false) {
                let placeholder = Expr::NilLit(self.span());
                (Some(ty), placeholder, true)
            } else {
                (None, self.parse_expr()?, false)
            };
            fields.push(FieldDef {
                name: fname,
                type_annotation: type_ann,
                default_value: fval,
                is_indexed: false,
                is_owned,
                required,
            });
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(ResourceDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            transient: false,
            version,
            fields,
            ownership,
        })
    }

    /// `fname: type` followed directly by the field separator — an
    /// annotation-only (required) field. Returns the parsed type and
    /// consumes it on success; restores the cursor otherwise.
    ///
    /// Bare uppercase identifiers stay DEFAULT VALUES (`speed: MAX_SPEED`
    /// is a constant reference, not a type) — only unambiguous type syntax
    /// counts: primitives, `entity`, unions, generics, tuples, fn types.
    fn try_annotation_only_field(&mut self, allow_named: bool) -> Option<TypeExpr> {
        let saved_pos = self.pos;
        match self.parse_type() {
            Ok(ty)
                if (self.check(TokenType::Comma) || self.check(TokenType::RBrace))
                    && (allow_named || Self::unambiguous_type_syntax(&ty)) =>
            {
                Some(ty)
            }
            _ => {
                self.pos = saved_pos;
                None
            }
        }
    }

    fn unambiguous_type_syntax(ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Named(n) => matches!(
                n.as_str(),
                "int" | "float" | "str" | "bool" | "any" | "entity" | "nil" | "list" | "map"
            ),
            _ => true, // unions, generics, tuples, fn types can't be values
        }
    }

    fn is_component_type_annotation(&mut self) -> bool {
        let saved_pos = self.pos;
        let is_type = match self.parse_type() {
            Ok(_) => self.check(TokenType::Assign),
            Err(_) => false,
        };
        self.pos = saved_pos;
        is_type
    }

    fn parse_entity_decl(&mut self) -> Result<EntityDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Entity)?;
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LBrace)?;
        let mut components = Vec::new();
        while !self.check(TokenType::RBrace) {
            if self.looks_like_component_init() {
                components.push(ComponentEntry::Init(self.parse_component_init()?));
            } else {
                components.push(ComponentEntry::Expr(self.parse_expr()?));
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(EntityDecl {
            id: self.next_id(),
            span,
            name,
            components,
            is_pub: false,
        })
    }

    fn parse_phase_decl(&mut self) -> Result<PhaseDecl, ParseError> {
        let span = self.span();
        self.advance(); // consume "phase" ident
        let name = self.expect_ident_text()?;
        // A phase is a declaration body and therefore has one canonical
        // brace-delimited form. Schedule invocation lists remain bracketed.
        self.expect(TokenType::LBrace)?;
        let mut systems = Vec::new();
        while !self.check(TokenType::RBrace) {
            systems.push(self.expect_ident_text()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(PhaseDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            systems,
            serial: false,
        })
    }

    pub(super) fn parse_component_init(&mut self) -> Result<ComponentInit, ParseError> {
        let span = self.span();
        let mut comp_name = self.expect_ident_text()?;
        if self.check(TokenType::Dot) {
            self.advance();
            comp_name.push('.');
            comp_name.push_str(&self.expect_ident_text()?);
        }
        if self.check(TokenType::DColon) {
            self.advance();
            comp_name.push_str("::");
            comp_name.push_str(&self.expect_ident_text()?);
            // State initialization in entity doesn't have braces
            return Ok(ComponentInit {
                id: self.next_id(),
                span,
                comp_name,
                fields: Vec::new(),
            });
        }
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let fname = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let fval = self.parse_expr()?;
            fields.push((fname, fval));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(ComponentInit {
            id: self.next_id(),
            span,
            comp_name,
            fields,
        })
    }

    fn parse_state_decl(&mut self) -> Result<StateDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::State)?;
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LBrace)?;
        let mut states = Vec::new();
        while !self.check(TokenType::RBrace) {
            states.push(self.parse_state_def()?);
        }
        self.expect(TokenType::RBrace)?;
        Ok(StateDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            states,
        })
    }

    fn parse_state_def(&mut self) -> Result<StateDef, ParseError> {
        let span = self.span();
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LBrace)?;
        let mut transitions = Vec::new();
        while !self.check(TokenType::RBrace) {
            self.expect(TokenType::On)?;
            let mut event = self.expect_ident_text()?;
            if self.check(TokenType::Dot) {
                self.advance();
                event.push('.');
                event.push_str(&self.expect_ident_text()?);
            }
            self.expect(TokenType::Arrow)?;
            let target = self.expect_ident_text()?;
            let guard = if self.check(TokenType::When) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            transitions.push((event, target, guard));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(StateDef {
            id: self.next_id(),
            span,
            name,
            transitions,
        })
    }

    fn parse_type_decl_or_alias(&mut self) -> Result<Decl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Type)?;
        let name = self.expect_ident_text()?;
        let type_params = self.parse_type_params()?;
        if self.check(TokenType::Assign) {
            self.advance();
            let target = self.parse_type()?;
            return Ok(Decl::TypeAlias(TypeAliasDecl {
                id: self.next_id(),
                span,
                name,
                type_params,
                target,
                is_pub: false,
            }));
        }
        self.expect(TokenType::LBrace)?;
        let mut variants = Vec::new();
        while !self.check(TokenType::RBrace) {
            let vname = self.expect_ident_text()?;
            self.expect(TokenType::LBrace)?;
            let mut fields = Vec::new();
            let mut annotations = Vec::new();
            while !self.check(TokenType::RBrace) {
                let fname = self.expect_field_name()?;
                self.expect(TokenType::Colon)?;
                // Variant fields are `name: default` — the default value also
                // fixes the field's type (spec 2.4) — NOT `name: Type =
                // default` like component/struct/resource. Spec 2.4 promises a
                // targeted diagnostic for that wrong turn rather than a bare
                // "Expected identifier, got Assign" (dogfood feature seq 56).
                // Deliver it, and name the working spelling for a field that
                // has no natural default (a recursive/self-referential field).
                if self.is_component_type_annotation() {
                    let tok = self.peek();
                    let tyname = tok.value.as_str().unwrap_or("Type").to_string();
                    let (line, col) = (tok.line, tok.col);
                    return Err(ParseError {
                        message: format!(
                            "variant field '{f}' is written `{f}: default` (the default value \
                             also fixes the field's type), not `{f}: {t} = default` as in \
                             component/struct/resource. For a field with no natural default — \
                             a recursive or self-referential field, e.g. a tree node — write \
                             just the type name: `{f}: {t}`.",
                            f = fname,
                            t = tyname
                        ),
                        line,
                        col,
                    });
                }
                // `name: type` (annotation, no default — `target: entity`)
                // vs `name: value` (default). Bare idents stay values so
                // type-param witnesses (`value: T`) keep working.
                match self.try_annotation_only_field(false) {
                    Some(ty) => {
                        let field_span = self.span();
                        annotations.push((fname.clone(), ty));
                        fields.push((fname, Expr::NilLit(field_span)));
                    }
                    None => {
                        let fval = self.parse_expr()?;
                        fields.push((fname, fval));
                    }
                }
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.expect(TokenType::RBrace)?;
            variants.push(VariantDefNode {
                name: vname,
                fields,
                annotations,
            });
        }
        self.expect(TokenType::RBrace)?;
        Ok(Decl::Type(TypeDeclNode {
            id: self.next_id(),
            span,
            name,
            type_params,
            variants,
            is_pub: false,
        }))
    }

    fn parse_event_decl(&mut self) -> Result<EventDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Event)?;
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let fname = self.expect_field_name()?;
            let type_ann = if self.check(TokenType::Colon) {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };
            fields.push((fname, type_ann));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RBrace)?;
        Ok(EventDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            fields,
        })
    }

    fn parse_migration_decl(&mut self) -> Result<MigrationDecl, ParseError> {
        let span = self.span();
        self.advance(); // consume `migrate`
        let component = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let param_name = self.expect_ident_text()?;
        // `migrate X(old, from_version)` — the optional second parameter
        // binds the save's declared schema version for X as an int
        // (dogfood feature seq 69 IDEA 03).
        let version_param = if self.check(TokenType::Comma) {
            self.advance();
            Some(self.expect_ident_text()?)
        } else {
            None
        };
        self.expect(TokenType::RParen)?;
        let body = self.parse_block()?;
        Ok(MigrationDecl {
            id: self.next_id(),
            span,
            component,
            param_name,
            version_param,
            body,
        })
    }
}
