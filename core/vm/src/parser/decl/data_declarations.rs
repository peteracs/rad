impl Parser {
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

    fn parse_data_decl(
        &mut self,
        kind: DataKind,
        allow_indexed: bool,
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
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        let mut indexed_fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            // `indexed` is only the marker when a field name follows it —
            // `indexed: int = 0` is a field literally named "indexed".
            let is_indexed = allow_indexed
                && self.check(TokenType::Indexed)
                && self.peek_at(1).ty != TokenType::Colon;
            if is_indexed {
                self.advance();
            }
            let fname = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let (type_ann, fval, required) = if self.is_component_type_annotation() {
                let ty = self.parse_type()?;
                self.expect(TokenType::Assign)?;
                let expr = self.parse_expr()?;
                (Some(ty), expr, false)
            } else if let Some(ty) = self.try_annotation_only_field() {
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
                required,
            });
            if is_indexed {
                indexed_fields.push(field_name);
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
        })
    }

    fn parse_resource_decl(&mut self) -> Result<ResourceDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Resource)?;
        let name = self.expect_ident_text()?;
        let version = self.try_parse_schema_version();
        self.expect(TokenType::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(TokenType::RBrace) {
            let fname = self.expect_field_name()?;
            self.expect(TokenType::Colon)?;
            let (type_ann, fval, required) = if self.is_component_type_annotation() {
                let ty = self.parse_type()?;
                self.expect(TokenType::Assign)?;
                let expr = self.parse_expr()?;
                (Some(ty), expr, false)
            } else if let Some(ty) = self.try_annotation_only_field() {
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
        })
    }

    /// `fname: type` followed directly by the field separator — an
    /// annotation-only (required) field. Returns the parsed type and
    /// consumes it on success; restores the cursor otherwise.
    ///
    /// Bare uppercase identifiers stay DEFAULT VALUES (`speed: MAX_SPEED`
    /// is a constant reference, not a type) — only unambiguous type syntax
    /// counts: primitives, `entity`, unions, generics, tuples, fn types.
    fn try_annotation_only_field(&mut self) -> Option<TypeExpr> {
        let saved_pos = self.pos;
        match self.parse_type() {
            Ok(ty)
                if (self.check(TokenType::Comma) || self.check(TokenType::RBrace))
                    && Self::unambiguous_type_syntax(&ty) =>
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
        // spec §3.5.1, the changelog, and the guide all spell this with
        // brackets (`phase P [A, B]`, matching `schedule [...]`), but the
        // parser historically only accepted braces. Accept either delimiter
        // and require the matching closer so both forms parse identically.
        let (open, close) = if self.check(TokenType::LBracket) {
            (TokenType::LBracket, TokenType::RBracket)
        } else {
            (TokenType::LBrace, TokenType::RBrace)
        };
        self.expect(open)?;
        let mut systems = Vec::new();
        while !self.check(close) {
            systems.push(self.expect_ident_text()?);
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(close)?;
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
                match self.try_annotation_only_field() {
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
