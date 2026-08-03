use super::*;

impl Parser {
    pub(super) fn parse_declaration(&mut self) -> Result<Decl, ParseError> {
        let is_pub = if self.check(TokenType::Pub) {
            self.advance();
            true
        } else {
            false
        };

        if self.check(TokenType::Component) {
            let mut decl = self.parse_data_decl(DataKind::Component, true)?;
            decl.is_pub = is_pub;
            return Ok(Decl::Component(decl));
        }
        if self.check(TokenType::Resource) {
            let mut decl = self.parse_resource_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Resource(decl));
        }
        // `transient resource Name { … }` — soft keyword: runtime state
        // excluded from world_digest()/save_world() (tapes, caches)
        if self.check_ident_text("transient") && self.peek_at(1).ty == TokenType::Resource {
            self.advance();
            let mut decl = self.parse_resource_decl()?;
            decl.is_pub = is_pub;
            decl.transient = true;
            return Ok(Decl::Resource(decl));
        }
        if self.check(TokenType::Struct) {
            let mut decl = self.parse_data_decl(DataKind::Struct, false)?;
            decl.is_pub = is_pub;
            return Ok(Decl::Struct(decl));
        }
        if self.check_ident_text("intent")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::LBrace
        {
            let mut decl = self.parse_intent_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Intent(decl));
        }
        if self.check_ident_text("law")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::LParen
        {
            let mut decl = self.parse_law_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Law(decl));
        }
        if self.check_ident_text("resolver")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::For
        {
            let mut decl = self.parse_resolver_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Resolver(decl));
        }
        if self.check_ident_text("constraint")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::For
        {
            let mut decl = self.parse_constraint_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Constraint(decl));
        }
        if self.check(TokenType::Entity) && self.peek_at(1).ty == TokenType::Ident {
            let mut decl = self.parse_entity_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Entity(decl));
        }
        if self.check(TokenType::State) {
            let mut decl = self.parse_state_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::State(decl));
        }
        if self.check(TokenType::System) {
            let mut decl = self.parse_system_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::System(decl));
        }
        if self.check(TokenType::On)
            || (self.check(TokenType::Async) && self.peek_at(1).ty == TokenType::On)
        {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on event handlers".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Ok(Decl::OnHandler(self.parse_on_handler()?));
        }
        if self.check(TokenType::Pure) {
            let mut decl = self.parse_pure_fn()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Fn(decl));
        }
        if self.is_effect_annotated_fn() {
            let mut decl = self.parse_effect_fn()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Fn(decl));
        }
        if self.check(TokenType::Event) {
            let mut decl = self.parse_event_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Event(decl));
        }
        if self.check(TokenType::Fn)
            || (self.check(TokenType::Async) && self.peek_at(1).ty == TokenType::Fn)
        {
            let mut decl = self.parse_fn_decl()?;
            decl.is_pub = is_pub;
            return Ok(Decl::Fn(decl));
        }
        if self.check(TokenType::Type) {
            let mut decl = self.parse_type_decl_or_alias()?;
            match &mut decl {
                Decl::Type(t) => t.is_pub = is_pub,
                Decl::TypeAlias(a) => a.is_pub = is_pub,
                _ => {}
            }
            return Ok(decl);
        }
        if self.check(TokenType::Use) {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on `use` statements".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Ok(Decl::Use(self.parse_use()?));
        }
        // Soft keyword: `migrate Component(old) { ... }` (schema migration,
        // list item #5). Only treated as a declaration when followed by an
        // identifier and `(` so `migrate` stays usable as a plain name.
        if self.check(TokenType::Ident)
            && self.peek().value.as_str() == Some("migrate")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(2).ty == TokenType::LParen
        {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on migrations".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Ok(Decl::Migration(self.parse_migration_decl()?));
        }
        // Soft keywords: `serial phase P [ ... ]` — a phase whose member
        // systems never share a parallel batch with each other (dogfood
        // feature seq 83). Only treated as a declaration when the full
        // `serial phase <Name>` shape is present, so `serial` stays usable
        // as a plain name.
        if self.check(TokenType::Ident)
            && self.peek().value.as_str() == Some("serial")
            && self.peek_at(1).ty == TokenType::Ident
            && self.peek_at(1).value.as_str() == Some("phase")
            && self.peek_at(2).ty == TokenType::Ident
        {
            self.advance(); // consume `serial`
            let mut decl = self.parse_phase_decl()?;
            decl.serial = true;
            decl.is_pub = is_pub;
            return Ok(Decl::Phase(decl));
        }
        if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("phase") {
            let next = self.peek_at(1);
            if next.ty == TokenType::Ident {
                let mut decl = self.parse_phase_decl()?;
                decl.is_pub = is_pub;
                return Ok(Decl::Phase(decl));
            }
        }
        if self.check(TokenType::Ident) && self.peek().value.as_str() == Some("test") {
            if is_pub {
                return Err(ParseError {
                    message: "`pub` is not allowed on tests".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            let next = self.peek_at(1);
            if next.ty == TokenType::String || next.ty == TokenType::Ident {
                return Ok(Decl::Test(self.parse_test_decl()?));
            }
        }
        if is_pub {
            // `pub let NAME = ...` — a module constant exported like a pub fn.
            if self.check(TokenType::Let) {
                let stmt = self.parse_statement()?;
                if let Stmt::Let(mut l) = stmt {
                    if l.tuple_destructure || l.names.len() != 1 {
                        return Err(ParseError {
                            message: "`pub let` requires a single name (no tuple destructuring)"
                                .to_string(),
                            line: l.span.line,
                            col: l.span.col,
                        });
                    }
                    if l.mutable {
                        return Err(ParseError {
                            message: "`pub let` must be immutable — exported module state would hide shared mutation; use a resource for shared mutable state".to_string(),
                            line: l.span.line,
                            col: l.span.col,
                        });
                    }
                    l.is_pub = true;
                    return Ok(Decl::Stmt(Stmt::Let(l)));
                }
                return Err(ParseError {
                    message: "`pub` is only allowed on top-level declarations".to_string(),
                    line: self.peek().line,
                    col: self.peek().col,
                });
            }
            return Err(ParseError {
                message: "`pub` is only allowed on top-level declarations".to_string(),
                line: self.peek().line,
                col: self.peek().col,
            });
        }
        Ok(Decl::Stmt(self.parse_statement()?))
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

    fn parse_pure_fn(&mut self) -> Result<FnDecl, ParseError> {
        self.expect(TokenType::Pure)?;
        let mut decl = self.parse_fn_decl()?;
        decl.is_pure = true;
        Ok(decl)
    }

    fn effect_name_at(&self, offset: usize) -> Option<&'static str> {
        let tok = self.peek_at(offset);
        match tok.ty {
            TokenType::Event => Some("event"),
            TokenType::Ident => match tok.value.as_str()? {
                "io" => Some("io"),
                "ecs" => Some("ecs"),
                "readonly" => Some("readonly"),
                "event" => Some("event"),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_effect_annotated_fn(&self) -> bool {
        let mut offset = 0;
        if self.effect_name_at(offset).is_none() {
            return false;
        }
        while self.effect_name_at(offset).is_some() {
            offset += 1;
        }
        self.peek_at(offset).ty == TokenType::Fn
    }

    fn parse_effect_fn(&mut self) -> Result<FnDecl, ParseError> {
        let mut effects = Vec::new();
        while let Some(effect) = self.effect_name_at(0) {
            effects.push(effect.to_string());
            self.advance();
        }
        let mut decl = self.parse_fn_decl()?;
        decl.effects = effects;
        Ok(decl)
    }

    fn parse_system_decl(&mut self) -> Result<SystemDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::System)?;
        let name = self.expect_ident_text()?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut accum_params: Vec<String> = Vec::new();
        while !self.check(TokenType::RParen) {
            let first = self.expect_ident_text()?;
            if !self.check(TokenType::Colon) {
                // Bare component name: a type-only filter param. The system
                // matches entities carrying it without binding the data —
                // the tag-component idiom (`system recompute(StatsDirty, ...)`).
                let mut ptype = first;
                if self.check(TokenType::Dot) {
                    self.advance();
                    ptype.push('.');
                    ptype.push_str(&self.expect_ident_text()?);
                }
                params.push((format!("_q{}", params.len()), false, ptype));
                if self.check(TokenType::Comma) {
                    self.advance();
                }
                continue;
            }
            let pname = first;
            self.expect(TokenType::Colon)?;
            let mut is_accum = false;
            // `p: accum R` — writable like `mut`, but parallel workers'
            // contributions are FOLDED per numeric field instead of
            // last-write-wins (dogfood feature seq 83 IDEA 02). Soft
            // keyword: only when another identifier (the type) follows, so
            // a type literally named `accum` still works as `p: accum`.
            let is_mut = if self.check(TokenType::Mut) {
                self.advance();
                true
            } else if self.check(TokenType::Ident)
                && self.peek().value.as_str() == Some("accum")
                && self.peek_at(1).ty == TokenType::Ident
            {
                self.advance();
                is_accum = true;
                true
            } else {
                false
            };
            let mut ptype = self.expect_ident_text()?;
            if self.check(TokenType::Dot) {
                self.advance();
                ptype.push('.');
                ptype.push_str(&self.expect_ident_text()?);
            }
            if is_accum {
                accum_params.push(pname.clone());
            }
            params.push((pname, is_mut, ptype));
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen)?;
        let mut after = Vec::new();
        let mut before = Vec::new();
        while self.check_ident_text("after") || self.check_ident_text("before") {
            if self.check_ident_text("after") {
                self.advance();
                let mut name = self.expect_ident_text()?;
                if self.check(TokenType::Dot) {
                    self.advance();
                    name.push('.');
                    name.push_str(&self.expect_ident_text()?);
                }
                after.push(name);
                while self.check(TokenType::Comma) {
                    self.advance();
                    let mut name = self.expect_ident_text()?;
                    if self.check(TokenType::Dot) {
                        self.advance();
                        name.push('.');
                        name.push_str(&self.expect_ident_text()?);
                    }
                    after.push(name);
                }
            } else if self.check_ident_text("before") {
                self.advance();
                let mut name = self.expect_ident_text()?;
                if self.check(TokenType::Dot) {
                    self.advance();
                    name.push('.');
                    name.push_str(&self.expect_ident_text()?);
                }
                before.push(name);
                while self.check(TokenType::Comma) {
                    self.advance();
                    let mut name = self.expect_ident_text()?;
                    if self.check(TokenType::Dot) {
                        self.advance();
                        name.push('.');
                        name.push_str(&self.expect_ident_text()?);
                    }
                    before.push(name);
                }
            }
        }
        let body = self.parse_block()?;
        Ok(SystemDecl {
            id: self.next_id(),
            span,
            name,
            is_pub: false,
            params,
            accum_params,
            body,
            after,
            before,
        })
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

    fn parse_on_handler(&mut self) -> Result<OnHandler, ParseError> {
        let span = self.span();
        let is_async = if self.check(TokenType::Async) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::On)?;
        let mut event_name = self.expect_ident_text()?;
        if self.check(TokenType::Dot) {
            self.advance();
            event_name.push('.');
            event_name.push_str(&self.expect_ident_text()?);
        }
        let once = if self.check(TokenType::Once) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::LParen)?;
        let param_name = self.expect_ident_text()?;
        self.expect(TokenType::RParen)?;
        let guard = if self.check(TokenType::When)
            || (self.check(TokenType::Ident) && self.peek().value.as_str() == Some("where"))
        {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        let has_guard = guard.is_some();
        let mut body = self.parse_block()?;
        if let Some(cond) = guard {
            let cond_span = cond.span().clone();
            let mut then_stmts = Vec::new();
            if once {
                then_stmts.push(Stmt::OnceGuardPass(cond_span.clone()));
            }
            then_stmts.extend(body.stmts);
            let then_block = Block {
                id: body.id,
                span: body.span,
                stmts: then_stmts,
            };
            let wrapped_if = Stmt::If(IfStmt {
                id: self.next_id(),
                span: cond_span.clone(),
                condition: cond,
                then_block,
                else_block: None,
            });
            body = Block {
                id: self.next_id(),
                span: cond_span,
                stmts: vec![wrapped_if],
            };
        }
        Ok(OnHandler {
            id: self.next_id(),
            span,
            event_name,
            param_name,
            body,
            once,
            is_async,
            has_guard,
        })
    }

    fn parse_fn_decl(&mut self) -> Result<FnDecl, ParseError> {
        let span = self.span();
        let is_async = if self.check(TokenType::Async) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::Fn)?;
        let name = self.expect_ident_text()?;
        let type_params = self.parse_type_params()?;
        self.expect(TokenType::LParen)?;
        let mut params = Vec::new();
        let mut param_muts = Vec::new();
        let mut param_types = Vec::new();
        while !self.check(TokenType::RParen) {
            let is_mut = self.check(TokenType::Mut);
            if is_mut {
                self.advance();
            }
            param_muts.push(is_mut);
            params.push(self.expect_ident_text()?);
            if self.check(TokenType::Colon) {
                self.advance();
                let ty = self.parse_type()?;
                param_types.push(Some(ty));
            } else {
                param_types.push(None);
            }
            if self.check(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen)?;
        let return_type = if self.check(TokenType::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        let body = self.parse_block()?;
        self.ensure_fn_param_alignment(&span, params.len(), param_types.len())?;
        Ok(FnDecl {
            id: self.next_id(),
            span,
            name,
            type_params,
            params,
            param_muts,
            param_types,
            return_type,
            body,
            is_pure: false,
            is_pub: false,
            is_async,
            effects: vec![],
        })
    }

    fn parse_type_params(&mut self) -> Result<Vec<String>, ParseError> {
        let mut type_params = Vec::new();
        if self.check(TokenType::Lt) {
            self.advance();
            while !self.check(TokenType::Gt) {
                type_params.push(self.expect_ident_text()?);
                if self.check(TokenType::Comma) {
                    self.advance();
                }
            }
            self.expect(TokenType::Gt)?;
        }
        Ok(type_params)
    }

    fn parse_test_decl(&mut self) -> Result<TestDecl, ParseError> {
        let span = self.span();
        self.expect(TokenType::Ident)?; // consume "test"

        let name = if self.check(TokenType::String) {
            let tok = self.peek().clone();
            self.advance();
            self.token_str_value(&tok, "test name")?.to_string()
        } else {
            self.expect_ident_text()?
        };

        // `for` lexes as a keyword token, never as an Ident: matching it as
        // an Ident made the documented property form unparseable
        // ("Expected LBrace, got For").
        let is_property = self.check(TokenType::For);

        let mut generators = Vec::new();
        if is_property {
            self.advance(); // consume "for"
            loop {
                let gname = self.expect_ident_text()?;
                self.expect(TokenType::In)?;
                let gexpr = self.parse_expr()?;
                generators.push((gname, gexpr));
                if self.check(TokenType::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        let mut body = self.parse_block()?;

        // Desugar the property form here, like `for … where` does: the body
        // runs once per generated value, i.e. wrapped in one loop per
        // generator (first generator outermost, cartesian product). The
        // checker then types each variable as the generator's ELEMENT type
        // and the compiler emits ordinary loops — no downstream special
        // case, so `generators` is handed on empty.
        for (gname, gexpr) in generators.into_iter().rev() {
            let loop_span = body.span.clone();
            body = Block {
                id: self.next_id(),
                span: loop_span.clone(),
                stmts: vec![Stmt::For(ForStmt {
                    id: self.next_id(),
                    span: loop_span,
                    bindings: vec![gname],
                    destructure_bindings: None,
                    iterable: gexpr,
                    body,
                })],
            };
        }

        Ok(TestDecl {
            id: self.next_id(),
            span,
            name,
            body,
            is_property,
            generators: Vec::new(),
        })
    }
}
